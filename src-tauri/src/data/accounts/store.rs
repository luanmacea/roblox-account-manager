/// Material de criptografia derivado uma única vez por unlock.
///
/// Por quê: `crypto::encrypt` sorteia um salt novo e chama `derive_key`
/// (argon2i, `OPSLIMIT_MODERATE`/`MEMLIMIT_MODERATE` ≈ 256 MiB) **a cada**
/// chamada. Como `save()` segura o lock de contas e os comandos Tauri síncronos
/// rodam na thread principal, mover N contas de grupo congelava a interface N
/// vezes. Agora o argon2 roda uma vez, no unlock.
///
/// Formato e segurança: o arquivo continua sendo
/// `RAM_HEADER | salt(16) | nonce(24) | ciphertext`, byte a byte igual ao que
/// `crypto::encrypt` produzia, e `crypto::decrypt` segue lendo normalmente. A
/// única diferença é que o salt passa a ser sorteado uma vez por unlock em vez
/// de uma vez por gravação. Isso não enfraquece nada: o salt existe para impedir
/// pré-computação de dicionário entre alvos diferentes (e ele continua aleatório
/// e rotacionado a cada unlock/mudança de senha); o valor que jamais pode se
/// repetir sob a mesma chave é o **nonce**, e esse continua sendo sorteado a
/// cada gravação. Reaproveitar a chave sem duplicar a montagem aqui exigiria um
/// `encrypt_with_key` em `data/crypto.rs`; enquanto aquele arquivo não puder ser
/// tocado, a montagem fica neste módulo.
/// Problema com o `AccountData.key` que a UI precisa mostrar.
///
/// **Estruturado de propósito.** A frase que o usuário lê tem que passar por
/// `t()` e pelo `i18n:extract` (Global Constraint 8), então o backend manda o
/// *código* e o *caminho*, e o catálogo tem a frase de cada código. Uma string em
/// inglês montada aqui nunca seria traduzida.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultKeyWarning {
    /// `writeFailed` | `writeFailedTransient` | `weakWrapper`.
    pub code: String,
    pub path: String,
    /// Detalhe técnico do SO, para o usuário poder reportar. Nunca tem segredo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl VaultKeyWarning {
    fn write_failed(path: &std::path::Path, detail: &str, transient: bool) -> Self {
        Self {
            code: if transient {
                "writeFailedTransient".to_string()
            } else {
                "writeFailed".to_string()
            },
            path: path.display().to_string(),
            detail: Some(detail.to_string()),
        }
    }

    fn weak_wrapper(path: &std::path::Path) -> Self {
        Self {
            code: "weakWrapper".to_string(),
            path: path.display().to_string(),
            detail: None,
        }
    }

    /// A migração para o formato cifrado falhou **fora** do arquivo de chave (a
    /// cópia de segurança ou a regravação do vault). O arquivo continua em texto
    /// puro, com o cookie de todas as contas legível, e antes disto **nada**
    /// aparecia: o `load()` devolvia `Err`, o `lib.rs` fazia `eprintln!`, o
    /// `needs_password()` era `false` e a tela dizia "Device Key". O app subia
    /// normal mentindo sobre o estado do arquivo.
    fn migration_failed(path: &std::path::Path, detail: &str) -> Self {
        Self {
            code: "migrationFailed".to_string(),
            path: path.display().to_string(),
            detail: Some(detail.to_string()),
        }
    }

    /// Gravou, mas sem confirmação do `fsync`.
    fn sync_unconfirmed(path: &std::path::Path) -> Self {
        Self {
            code: "syncUnconfirmed".to_string(),
            path: path.display().to_string(),
            detail: None,
        }
    }

    /// Peso do aviso. O slot é único, então sem isto um aviso brando **rebaixava**
    /// um grave por chegar depois — `writeFailed` ("faça backup agora") virava
    /// `syncUnconfirmed` ("pode perder a última alteração") no mesmo `save_locked`,
    /// e os dois co-ocorrem justamente no mesmo tipo de volume. Sub-avisar é o que
    /// custa contas.
    ///
    /// Cada aviso grave tem um caminho de limpeza pelo **escopo** dele
    /// (`clear_key_warning_for`), então nunca-rebaixar não deixa nada preso.
    fn severity(&self) -> u8 {
        match self.code.as_str() {
            "writeFailed" | "migrationFailed" => 2,
            _ => 1,
        }
    }
}

/// De onde veio o segredo que cifra o vault nesta sessão.
///
/// Importa porque as duas fontes têm regras diferentes: a senha do usuário é
/// pedida na tela de senha e o `.key` não existe; a chave do aparelho abre o
/// vault sozinha e vive naquele arquivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VaultSecret {
    /// Senha digitada pelo usuário (Pass Lock).
    UserPassword,
    /// Chave mestra do arquivo `.key` ao lado do vault.
    DeviceKey,
}

struct SessionKey {
    /// SHA-512 da senha. Barato, e ainda necessário para decriptar arquivos
    /// gravados com outros salts (o próprio arquivo lido no unlock, por ex.).
    password_hash: Vec<u8>,
    salt: crypto::Salt,
    key: crypto::Key,
    secret: VaultSecret,
    /// A chave mestra de 32 bytes, só quando o segredo é a chave do aparelho.
    ///
    /// Guardar isto é o que permite **recriar o `.key`** se ele desaparecer com o
    /// app rodando; com só o hash derivado, nada no processo poderia reconstruir o
    /// arquivo. Nunca sai daqui: não há `Debug`, não é serializado e nenhuma
    /// mensagem de erro a inclui.
    master: Option<Vec<u8>>,
}

impl SessionKey {
    /// Espera a senha já normalizada (com trim) pelo chamador, do mesmo jeito
    /// que `crypto::hash_password`.
    fn derive(password: &str) -> Result<Self, String> {
        Self::from_hash(
            crypto::hash_password(password),
            VaultSecret::UserPassword,
            None,
        )
    }

    /// A sessão da chave do aparelho, que carrega a chave mestra junto.
    fn from_master_key(master: Vec<u8>) -> Result<Self, String> {
        let hash = crate::data::vault_key::master_password_hash(&master);
        Self::from_hash(hash, VaultSecret::DeviceKey, Some(master))
    }

    /// Mesma montagem, mas a partir de um hash já pronto — é assim que a chave
    /// mestra do aparelho entra, já que ela não é uma senha digitada.
    fn from_hash(
        password_hash: Vec<u8>,
        secret: VaultSecret,
        master: Option<Vec<u8>>,
    ) -> Result<Self, String> {
        let salt = crypto::gen_salt();
        let key = crypto::derive_key(&password_hash, &salt)
            .map_err(|e| format!("Failed to derive key: {}", e))?;
        Ok(Self {
            password_hash,
            salt,
            key,
            secret,
            master,
        })
    }

    fn encrypt(&self, content: &str) -> Result<Vec<u8>, String> {
        if content.is_empty() {
            return Err("Failed to encrypt: Invalid encrypted data".to_string());
        }

        let nonce = crypto::gen_nonce();
        let ciphertext = crypto::seal(content.as_bytes(), &nonce, &self.key);

        let mut output =
            Vec::with_capacity(crypto::RAM_HEADER.len() + 16 + 24 + ciphertext.len());
        output.extend_from_slice(crypto::RAM_HEADER);
        output.extend_from_slice(&self.salt);
        output.extend_from_slice(&nonce);
        output.extend_from_slice(&ciphertext);
        Ok(output)
    }
}

pub struct AccountStore {
    accounts: Mutex<Vec<Account>>,
    /// `None` = o vault ainda não foi aberto (trancado, ou nem tentado).
    /// `Some` = há segredo em memória, de senha ou da chave do aparelho.
    /// Ordem de lock em todo o arquivo: `accounts` → `session`.
    session: Mutex<Option<SessionKey>>,
    file_path: PathBuf,
    /// Set when the on-disk file exists but could not be decoded. While set,
    /// `save()` refuses to write so an empty in-memory list never overwrites
    /// the user's accounts.
    load_failed: std::sync::atomic::AtomicBool,
    /// Motivo para recusar gravação até o app reiniciar, quando o **arquivo**
    /// está bom e é a memória que está velha (ver `lock_writes_until_restart`).
    write_block: Mutex<Option<String>>,
    /// Problema com o **arquivo de chave** que a UI precisa mostrar.
    ///
    /// `eprintln!` numa build GUI não vai a lugar nenhum, e o `.key` ruim é
    /// justamente o defeito que passa o dia inteiro invisível (o master está em
    /// memória) para virar lockout no boot seguinte. Isto existe para o usuário
    /// saber **no dia em que o arquivo fica ruim**.
    key_warning: Mutex<Option<VaultKeyWarning>>,
    /// Quem quer saber **na hora** que o aviso mudou (a janela, via `lib.rs`).
    ///
    /// Ler o slot quando a UI pede não basta: gravador de fundo — Auto Rejoin a
    /// cada ciclo, Watcher, servidor HTTP — muda o aviso sem a UI pedir nada.
    key_warning_watchers: Mutex<Vec<std::sync::mpsc::Sender<Option<VaultKeyWarning>>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OldAccountImportSummary {
    pub total: usize,
    pub added: usize,
    pub replaced: usize,
    pub skipped: usize,
}

const IMPORT_PASSWORD_REQUIRED: &str = "IMPORT_PASSWORD_REQUIRED";

impl AccountStore {
    pub fn new(file_path: PathBuf) -> Self {
        Self {
            accounts: Mutex::new(Vec::new()),
            session: Mutex::new(None),
            file_path,
            load_failed: std::sync::atomic::AtomicBool::new(false),
            write_block: Mutex::new(None),
            key_warning: Mutex::new(None),
            key_warning_watchers: Mutex::new(Vec::new()),
        }
    }

    /// Onde fica a chave mestra deste vault. Segue o **arquivo**, não um caminho
    /// fixo, então o modo portátil leva os dois juntos.
    fn key_file_path(&self) -> PathBuf {
        crate::data::vault_key::key_file_path_for(&self.file_path)
    }

    /// Cópia do vault em `AccountData.json.bak`, **antes** da migração.
    ///
    /// Erro aqui **aborta** a migração: sem rede de segurança não se troca o
    /// formato do arquivo que guarda as contas do usuário. Esta é a única cópia em
    /// **texto puro** que o app faz, ou seja, a única que abre sem chave nenhuma —
    /// e por isso a única que vale como rede. `set_password` não faz cópia (ver lá
    /// o porquê), então não existe mais um sufixo para escolher.
    fn backup_vault_file(&self) -> Result<(), String> {
        if !self.file_path.exists() {
            return Ok(());
        }
        let backup = self.file_path.with_extension("json.bak");
        fs::copy(&self.file_path, &backup).map_err(|e| {
            format!(
                "Failed to back up the account file to {}: {}",
                backup.display(),
                e
            )
        })?;
        Ok(())
    }

    /// A pasta de dados (onde ficam `AccountData.json` e `.key`) e a de
    /// backups, para as mensagens de bloqueio darem o caminho à mão.
    fn data_and_backups_dirs(&self) -> (std::path::PathBuf, std::path::PathBuf) {
        let data_dir = self
            .file_path
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_default();
        let backups_dir = data_dir.join(crate::BACKUPS_DIR_NAME);
        (data_dir, backups_dir)
    }

    /// Mensagem para o caso em que o vault existe, está cifrado e **nada** abre.
    ///
    /// A presença do `.key` separa os dois motivos, e eles pedem respostas
    /// diferentes do usuário: sem `.key` é vault de senha (digite a senha); com
    /// `.key` que não abre, a chave deste aparelho se perdeu (Windows
    /// reinstalado, perfil novo, arquivo trocado por antivírus) e o caminho é a
    /// cópia em `.json.bak` ou um backup do app.
    fn locked_vault_message(&self) -> String {
        let key_path = self.key_file_path();
        if !key_path.exists() {
            return "Password required for encrypted file".to_string();
        }

        // O dono lê isto na **tela de senha**, que só tem senha e Continue:
        // Settings não abre com as contas trancadas, e mandar "restaurar pelo
        // Settings" era mandar abrir uma tela que não abre. O caminho real é à
        // mão, e a ordem importa: com o app aberto nada é relido, e pôr os
        // arquivos do backup por cima sem tirar os de agora do lugar perde o
        // vault que talvez ainda abra no aparelho que o criou.
        let (data_dir, backups_dir) = self.data_and_backups_dirs();

        // Só citar o `.json.bak` quando ele **existe**: numa instalação que nasceu
        // cifrada esse arquivo nunca existiu, e mandar restaurá-lo é mandar a
        // pessoa caçar um arquivo inexistente exatamente no momento de pânico.
        let plain_backup = self.file_path.with_extension("json.bak");
        let put_back = if plain_backup.exists() {
            format!(
                "Put back either the copy at {} renamed to AccountData.json (plain text from before \
                 encryption: accounts added since then are not in it), or AccountData.json and \
                 AccountData.key from a backup zip in {}",
                plain_backup.display(),
                backups_dir.display()
            )
        } else {
            format!(
                "Put back AccountData.json and AccountData.key from a backup zip in {}",
                backups_dir.display()
            )
        };

        format!(
            "The account vault is encrypted with this device's key and that key could not be recovered ({}). \
             Nothing was deleted or overwritten. Settings does not open while the accounts are locked, so \
             restore by hand, in this order: 1. Close the app. 2. Move AccountData.json and AccountData.key \
             out of {} and keep them. 3. {}. 4. Open the app again. A backup only opens on the PC and Windows \
             user that made it. Instead of step 3, you can also open the app with both files moved out, \
             restore a backup in Settings > Misc > Backups and restart the app. To start over with no \
             accounts, just open the app after step 2.",
            key_path.display(),
            data_dir.display(),
            put_back
        )
    }

    /// Abre o `.key` e **regrava os dois embrulhos** com os identificadores de
    /// agora. É o único ponto que recupera a chave mestra.
    ///
    /// A regravação é **incondicional**, e isso é o ponto: o embrulho que não foi
    /// usado para abrir pode estar morto sem ninguém notar. O caso que custa as
    /// contas é o silencioso — o usuário renomeia o PC, o blob do aparelho fica
    /// preso ao nome antigo, **o DPAPI continua abrindo e nada parece errado**; um
    /// perfil recriado meses depois (justamente o caso para o qual o segundo
    /// embrulho existe) não encontra caminho nenhum. Conferir sem regravar
    /// custaria o mesmo argon2 que regravar, então não há motivo para só conferir.
    ///
    /// Falha na regravação **não** é fatal: a chave em memória continua boa e o
    /// `.key` de antes continua no disco.
    fn recover_and_refresh_master_key(&self) -> Option<Vec<u8>> {
        let key_path = self.key_file_path();
        let recovered = crate::data::vault_key::load_master_key(&key_path)?;
        self.refresh_key_file(&recovered.master);
        Some(recovered.master)
    }

    /// Regrava o `.key` com os embrulhos de agora e **registra o que deu errado**
    /// onde a UI consiga ler. Devolve `true` quando o arquivo ficou bom.
    fn refresh_key_file(&self, master: &[u8]) -> bool {
        let key_path = self.key_file_path();
        match crate::data::vault_key::store_master_key(
            &key_path,
            master,
            &crypto::primary_device_hash(),
        ) {
            Ok(health) => {
                if cfg!(target_os = "windows") && !health.dpapi_present {
                    // O `.key` ficou só com o embrulho fraco. Não é perda, mas é
                    // degradação — e degradar calado foi exatamente o problema.
                    self.set_key_warning(VaultKeyWarning::weak_wrapper(&key_path));
                } else if !health.synced {
                    self.set_key_warning(VaultKeyWarning::sync_unconfirmed(&key_path));
                } else {
                    // **Com escopo.** Este reparo resolve o `.key` e mais nada:
                    // limpar o slot inteiro aqui apagava um `migrationFailed` (que
                    // fala do `AccountData.json`) antes de a gravação do vault
                    // sequer acontecer. A invariante é que nenhum caminho limpa
                    // aviso de escopo alheio.
                    self.clear_key_warning_for(&key_path);
                }
                true
            }
            Err(e) => {
                // Transitório (antivírio/indexador segurando o handle) recebe o
                // tom brando: a gravação seguinte tenta de novo e limpa o aviso.
                // Alarmar alto num arquivo que está perfeito treina o usuário a
                // ignorar avisos, e aí a rede contra o lockout não vale nada.
                self.set_key_warning(VaultKeyWarning::write_failed(
                    &key_path,
                    &e.message,
                    e.transient,
                ));
                false
            }
        }
    }

    /// O mutex do aviso é **destravado do envenenamento** de propósito.
    ///
    /// `if let Ok(..)` descartava a escrita e `.lock().ok()` respondia "tudo em
    /// ordem" com o mutex envenenado: falha não observável **no canal que existe
    /// para observar falhas**. O dado aqui é um aviso, não invariante de
    /// segurança — recuperá-lo com `into_inner()` é sempre melhor que engolir.
    fn key_warning_slot(&self) -> std::sync::MutexGuard<'_, Option<VaultKeyWarning>> {
        self.key_warning
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn set_key_warning(&self, warning: VaultKeyWarning) {
        let mut slot = self.key_warning_slot();
        if let Some(current) = slot.as_ref() {
            // Idêntico: não repete nem torna a logar. Sem isto, um volume que
            // nunca confirma o `fsync` escrevia a mesma linha a cada gravação.
            if *current == warning {
                return;
            }
            // Nunca rebaixa a gravidade (ver `VaultKeyWarning::severity`).
            if current.severity() > warning.severity() {
                return;
            }
        }
        eprintln!("Aviso ({}): {}", warning.code, warning.path);
        *slot = Some(warning);
        self.publish_key_warning(slot.as_ref());
    }

    fn clear_key_warning(&self) {
        let mut slot = self.key_warning_slot();
        if slot.take().is_some() {
            self.publish_key_warning(None);
        }
    }

    /// Limpa o aviso **só se ele for sobre este arquivo**.
    ///
    /// O slot é único mas os avisos falam de arquivos diferentes: `writeFailed` e
    /// `weakWrapper` são do `.key`, `migrationFailed` é do `AccountData.json`.
    /// Limpar sem escopo fazia o braço "o `.key` está saudável" apagar a faixa
    /// vermelha que dizia que o vault continua em texto puro — e apagar **antes**
    /// de a gravação acontecer, então nem o sucesso justificava.
    fn clear_key_warning_for(&self, path: &std::path::Path) {
        let mut slot = self.key_warning_slot();
        let belongs = slot
            .as_ref()
            .is_some_and(|w| w.path == path.display().to_string());
        if belongs {
            *slot = None;
            self.publish_key_warning(None);
        }
    }

    /// Canal com cada mudança do aviso, **na ordem em que aconteceram**
    /// (`Some` = aviso novo, `None` = sumiu).
    ///
    /// O store só **manda** no canal — não bloqueia, então dá para fazer segurando
    /// o lock de contas, que é de onde o aviso muda (`save_locked`). Quem fala com
    /// a janela é outra thread, fora de qualquer lock do store
    /// (`forward_vault_key_warning`): um comando síncrono na thread principal pode
    /// estar esperando justamente o lock de contas.
    pub fn watch_key_warning(&self) -> std::sync::mpsc::Receiver<Option<VaultKeyWarning>> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.key_warning_watchers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(tx);
        rx
    }

    /// Conta a mudança para quem estiver ouvindo.
    ///
    /// Chamado **com o slot travado**, e é isso que mantém a ordem: duas threads
    /// mudando o aviso ao mesmo tempo publicam na mesma ordem em que mudaram o
    /// slot, então a UI termina no estado que o slot tem.
    fn publish_key_warning(&self, warning: Option<&VaultKeyWarning>) {
        let mut watchers = self
            .key_warning_watchers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Quem parou de ouvir sai da lista.
        watchers.retain(|tx| tx.send(warning.cloned()).is_ok());
    }

    /// A chave do aparelho não pôde ser montada e **não há sessão**: o
    /// `AccountData.json` continua em texto puro — ou vai nascer assim, no
    /// primeiro boot. É isso que a faixa tem que dizer, e por isso o
    /// `migrationFailed` **vence** aqui.
    ///
    /// `refresh_key_file` já deixou no slot um aviso sobre o `.key`, e os dois
    /// textos possíveis mentem neste caso: o transitório promete "tento de novo na
    /// próxima alteração", mas sem sessão o `save_locked` nem toca no `.key`; o
    /// `writeFailed` diz "pode não abrir depois de fechar", mas o arquivo está
    /// legível e abre. O que importa é o cookie de todas as contas legível no
    /// disco. Com sessão (senha, ou chave do aparelho já em uso) o arquivo não
    /// está em texto puro, e isto não mexe em nada.
    fn warn_left_in_plain_text(&self, detail: &str) {
        let no_session = self
            .session
            .lock()
            .map(|session| session.is_none())
            // Envenenado: sem saber, o aviso grave é o lado seguro.
            .unwrap_or(true);
        if no_session {
            self.set_key_warning(VaultKeyWarning::migration_failed(&self.file_path, detail));
        }
    }

    /// O que a UI mostra sobre o arquivo de chave. `None` = tudo em ordem.
    ///
    /// Estruturado (código + caminho), não frase pronta: a frase mora no catálogo
    /// de i18n e passa por `t()`, como todo texto que o usuário lê.
    pub fn vault_key_warning(&self) -> Option<VaultKeyWarning> {
        self.key_warning_slot().clone()
    }

    /// Recusa a operação quando a gravação está trancada.
    ///
    /// Checado na **entrada** de `set_password`: só olhar dentro de `save_locked`
    /// deixava "somente leitura" não ser literal — ele já mexia em arquivo (cópia,
    /// e a regravação do `.key`) antes de descobrir que não podia gravar.
    fn ensure_writable(&self) -> Result<(), String> {
        let block = self.write_block.lock().map_err(|e| e.to_string())?;
        match block.as_deref() {
            Some(reason) => Err(format!(
                "Refusing to write the account file: {}. Restart the app before changing accounts.",
                reason
            )),
            None => Ok(()),
        }
    }

    /// A gravação está trancada agora?
    ///
    /// Existe para quem **tranca** poder saber se a trava já era de outro: soltar
    /// uma trava alheia é o mesmo que nunca tê-la ligado.
    pub fn writes_locked(&self) -> bool {
        self.write_block
            .lock()
            .map(|slot| slot.is_some())
            // Mutex envenenado: assumir trancado é o lado seguro.
            .unwrap_or(true)
    }

    /// Libera a gravação depois de o arquivo ter sido **relido de verdade**.
    ///
    /// Contraparte de [`Self::lock_writes_until_restart`]. Existe porque a
    /// restauração de backup passou a trancar **por padrão**: é este o único ponto
    /// que destranca, e só quem releu o arquivo pode chamá-lo.
    pub fn allow_writes_after_reload(&self) {
        if let Ok(mut slot) = self.write_block.lock() {
            *slot = None;
        }
    }

    /// Recusa **toda** gravação até o app reiniciar.
    ///
    /// Diferente de `load_failed`: ali o arquivo em disco é que está ruim; aqui o
    /// arquivo está ótimo e é a **memória** que está velha. É o caso da
    /// restauração de backup — o segredo da sessão não é mais o do arquivo, e
    /// qualquer gravação (um launch já chama `mark_used`) cifraria o vault
    /// restaurado com o segredo antigo, deixando-o sem abrir no próximo boot.
    ///
    /// **Só volta depois de a gravação em andamento terminar.** Ligar a trava não
    /// basta: `save_locked` confere a trava no começo e só publica o arquivo no
    /// fim (fsync + troca atômica), e quem já passou pela checagem com ela aberta
    /// terminava **depois** — um ciclo do Auto Rejoin no meio do `mark_used`
    /// punha o vault de antes por cima do que a extração acabara de restaurar.
    /// Toda gravação segura `accounts` da checagem até a troca do arquivo, então
    /// tomar esse mutex antes de ligar a trava espera exatamente quem já estava
    /// gravando, e quem chegar depois encontra a trava ligada. A ordem
    /// `accounts` → `write_block` é a mesma de `save_locked`.
    pub fn lock_writes_until_restart(&self, reason: &str) {
        // Envenenado quer dizer que quem segurava morreu: não há gravação em
        // andamento a esperar, e a trava tem que ligar do mesmo jeito.
        let _writers = self
            .accounts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Ok(mut slot) = self.write_block.lock() {
            *slot = Some(reason.to_string());
        }
    }

    /// Estabelece a sessão da **chave do aparelho**, reaproveitando o `.key` que
    /// já existir e criando um novo só quando não houver nenhum.
    ///
    /// Idempotente de propósito: se o processo morrer entre gravar o `.key` e
    /// regravar o vault, a abertura seguinte cai aqui de novo, reencontra a mesma
    /// chave e termina a migração.
    fn ensure_device_session(&self) -> Result<(), String> {
        {
            let session = self.session.lock().map_err(|e| e.to_string())?;
            if session.is_some() {
                return Ok(());
            }
        }

        let session = self.build_device_session()?;
        let mut slot = self.session.lock().map_err(|e| e.to_string())?;
        *slot = Some(session);
        Ok(())
    }

    /// Pode existir vault cifrado a perder? **Erro conta como "sim".**
    ///
    /// Isto guarda a decisão de sortear uma chave mestra nova, que destrói a única
    /// cópia da antiga. `is_encrypted().unwrap_or(false)` era **fail-open** nessa
    /// guarda: "não consegui saber" virava "não existe". As duas leituras falham
    /// juntas com mais frequência do que parece — varredura de antivírus e
    /// rehidratação do OneDrive pegam a **pasta inteira**, então `.key` ilegível e
    /// `AccountData.json` ilegível são eventos correlacionados, não independentes.
    /// Se depois disso a regravação do vault falhasse com o `.key` novo já
    /// publicado, o resultado era lockout permanente de todas as contas.
    fn vault_may_hold_data(&self) -> bool {
        match self.is_encrypted() {
            Ok(encrypted) => encrypted,
            Err(_) => true,
        }
    }

    /// Monta a sessão da chave do aparelho sem tocar no que já está em memória.
    ///
    /// Separado de [`Self::ensure_device_session`] para `set_password(None)`
    /// poder falhar **sem** derrubar a sessão da senha que o usuário já tinha.
    fn build_device_session(&self) -> Result<SessionKey, String> {
        let key_path = self.key_file_path();
        let master = match self.recover_and_refresh_master_key() {
            Some(master) => master,
            // Só é proibido sortear chave nova quando **há vault cifrado a
            // perder**: aí um `.key` que não abre pode ser a única cópia da chave
            // daquele arquivo, e substituí-lo o tornaria ilegível para sempre.
            // Antes a condição era só `key_path.exists()`, o que travava também a
            // **instalação nova** quando havia qualquer coisa no lugar do `.key`
            // (um diretório, por exemplo) — sem vault nenhum para proteger.
            None if key_path.exists() && self.vault_may_hold_data() => {
                return Err(self.locked_vault_message());
            }
            None => {
                let master = crate::data::vault_key::generate_master_key();
                // O `.key` é gravado **antes** de o vault ser cifrado com ele: na
                // ordem contrária, uma falha aqui deixaria um vault que ninguém
                // abre. E passa por `refresh_key_file` justamente para o health e o
                // erro chegarem ao aviso da UI: este é o **primeiro boot** e o
                // `set_password(None)`, e antes os dois degradavam calados (o
                // health era descartado) ou morriam num `eprintln!` do `lib.rs`.
                if !self.refresh_key_file(&master) {
                    return Err(format!(
                        "The account key file ({}) could not be created, so the account file was \
                         left unencrypted. See the warning on screen.",
                        key_path.display()
                    ));
                }
                master
            }
        };

        SessionKey::from_master_key(master)
    }

    /// Os bytes no disco começam com um header RAM? Fato bruto do arquivo, usado
    /// pelas guardas de gravação — **não** é "tem senha" (ver
    /// [`Self::has_user_password`]).
    pub fn is_encrypted(&self) -> Result<bool, String> {
        if !self.file_path.exists() {
            return Ok(false);
        }

        let data =
            fs::read(&self.file_path).map_err(|e| format!("Failed to read account file: {}", e))?;

        Ok(crypto::is_encrypted(&data))
    }

    /// O vault é protegido por **senha do usuário**?
    ///
    /// É isso que a tela de criptografia mostra. Desde que o vault sem senha
    /// também é cifrado, `is_encrypted()` deixou de responder essa pergunta: ela
    /// é "true" nos dois casos. O sinal é o arquivo `.key` — ele existe quando a
    /// chave é do aparelho e é removido quando o usuário define uma senha.
    pub fn has_user_password(&self) -> Result<bool, String> {
        {
            let session = self.session.lock().map_err(|e| e.to_string())?;
            if let Some(session) = session.as_ref() {
                return Ok(session.secret == VaultSecret::UserPassword);
            }
        }
        if !self.is_encrypted()? {
            return Ok(false);
        }
        Ok(!self.key_file_path().exists())
    }

    /// O usuário precisa digitar a senha para o app abrir as contas?
    ///
    /// Depois de `load()`, "sim" significa: o arquivo está cifrado e nada em
    /// memória abre. Isso cobre tanto o vault de senha (caso normal) quanto o
    /// vault cuja chave de aparelho se perdeu — nos dois a tela de senha é a
    /// tela certa, e a mensagem de erro do unlock explica a diferença.
    pub fn needs_password(&self) -> Result<bool, String> {
        let session = self.session.lock().map_err(|e| e.to_string())?;
        if session.is_some() {
            return Ok(false);
        }
        drop(session);
        self.is_encrypted()
    }

    /// Abre o vault sem senha do usuário: chave do aparelho, ou JSON puro (que é
    /// migrado na hora).
    pub fn load(&self) -> Result<(), String> {
        let data = if self.file_path.exists() {
            fs::read(&self.file_path).map_err(|e| format!("Failed to read account file: {}", e))?
        } else {
            Vec::new()
        };

        if data.is_empty() {
            // Instalação nova (ou arquivo de 0 byte, que é um vazio legítimo): o
            // vault já nasce com a chave do aparelho, senão o primeiro `add`
            // gravaria JSON puro e a migração nunca aconteceria.
            //
            // Falhar aqui **não** trava o app: sem chave, o store segue sem
            // segredo e grava em texto puro, como antes desta mudança. É uma
            // proteção a menos, não um usuário sem acesso às contas — e a faixa
            // diz exatamente isso (`warn_left_in_plain_text`).
            return self.ensure_device_session().map_err(|e| {
                self.warn_left_in_plain_text(&e);
                e
            });
        }

        if crypto::is_encrypted(&data) {
            return self.load_encrypted(&data);
        }

        // JSON puro (ou DPAPI legado do RAM v3): ler e migrar.
        let accounts = match Self::decode_plain_or_legacy_accounts(&data) {
            Ok(accounts) => accounts,
            Err(e) => {
                self.load_failed
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                return Err(e);
            }
        };

        let mut store = self.accounts.lock().map_err(|e| e.to_string())?;
        *store = accounts;
        drop(store);
        self.mark_memory_fresh();

        self.migrate_plain_vault()
    }

    /// A memória acabou de ser lida do disco, então ela **não está velha**.
    ///
    /// É o que solta o `write_block`, e é por isso que a migração de um vault em
    /// texto puro restaurado de backup consegue rodar: o latch existe para impedir
    /// que memória velha sobrescreva o arquivo, e aqui a memória é o arquivo. Sem
    /// isto, a trava que a restauração liga bloqueava o `save_locked` da própria
    /// migração — `load()` devolvia "não foi possível reler" **depois** de ler com
    /// sucesso, e o caminho "texto puro recarrega na hora" ficava inalcançável.
    fn mark_memory_fresh(&self) {
        self.load_failed
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.allow_writes_after_reload();
    }

    /// Regrava um vault em texto puro como vault cifrado.
    ///
    /// A ordem existe para nenhuma queda no meio custar contas:
    /// 1. `.json.bak` — sem backup, não migra;
    /// 2. `.key` — a chave antes do arquivo que ela cifra;
    /// 3. regravação do vault, atômica (`save_locked`).
    ///
    /// Morrer entre 1 e 2, ou entre 2 e 3, deixa o vault **em texto puro** e a
    /// próxima abertura recomeça daqui com a mesma chave. Morrer dentro de 3 não
    /// existe: a troca é atômica, o arquivo é o de antes ou o de depois.
    fn migrate_plain_vault(&self) -> Result<(), String> {
        // **Toda** falha daqui tem que virar aviso na tela, não só `Err`.
        //
        // Este era o pior fail-open que sobrou: `load()` devolvia `Err`, o boot
        // fazia `eprintln!` (invisível em build GUI), o `mark_memory_fresh` já
        // tinha limpado o `load_failed`, `needs_password()` era `false` porque o
        // arquivo está em texto puro, e nenhum `key_warning` era setado. Resultado:
        // o app subia normal, a tela de criptografia dizia "Device Key", e o
        // `AccountData.json` continuava em texto puro com o `.ROBLOSECURITY` de
        // todas as contas — sem um sinal em lugar nenhum. É o mesmo padrão que a
        // Quebra 1 mandou matar, sobrevivendo no caminho principal desta tarefa.
        self.backup_vault_file().map_err(|e| {
            self.set_key_warning(VaultKeyWarning::migration_failed(&self.file_path, &e));
            e
        })?;

        if let Err(e) = self.ensure_device_session() {
            // Sem chave o arquivo fica como está — em texto puro, legível, mas
            // **inteiro**. Perder o arquivo é pior que ficar sem a criptografia.
            // E a faixa diz isso **por cima** do aviso sobre o `.key` que o
            // `refresh_key_file` deixou: antes o `migrationFailed` só entrava com
            // o slot vazio, e a faixa ficava falando do `.key`.
            self.warn_left_in_plain_text(&e);
            return Err(format!(
                "Accounts are loaded, but the file could not be encrypted and was left as-is: {}",
                e
            ));
        }

        let accounts = self.accounts.lock().map_err(|e| e.to_string())?;
        self.save_locked(&accounts).map_err(|e| {
            self.set_key_warning(VaultKeyWarning::migration_failed(&self.file_path, &e));
            e
        })
    }

    /// Abre um vault já cifrado sem senha do usuário.
    fn load_encrypted(&self, data: &[u8]) -> Result<(), String> {
        // Sessão já estabelecida (unlock anterior): usa o segredo que está lá.
        let session_hash = {
            let session = self.session.lock().map_err(|e| e.to_string())?;
            session.as_ref().map(|s| s.password_hash.clone())
        };
        if let Some(hash) = session_hash {
            let Ok(decrypted) = crypto::decrypt(data, &hash) else {
                // O segredo em memória não abre o arquivo que está no disco: a
                // memória está velha (arquivo restaurado por fora, por exemplo).
                // Latchar é obrigatório — sem isso a gravação seguinte cifraria o
                // arquivo novo com o segredo antigo e o próximo boot não abriria.
                self.load_failed
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                return Err(
                    "The account file on disk was not written by this session's key; \
                     refusing to touch it. Restart the app."
                        .to_string(),
                );
            };
            return self.commit_loaded(decrypted);
        }

        let key_path = self.key_file_path();
        if !key_path.exists() {
            // Sem arquivo de chave o vault é de senha. Não vale gastar um argon2
            // por candidato de aparelho a cada boot para descobrir isso.
            self.load_failed
                .store(true, std::sync::atomic::Ordering::SeqCst);
            return Err(self.locked_vault_message());
        }

        let Some(master) = self.recover_and_refresh_master_key() else {
            // Chave irrecuperável: **não** apagar, **não** regravar, **não**
            // travar o app. Só dizer o que houve e onde está a cópia.
            self.load_failed
                .store(true, std::sync::atomic::Ordering::SeqCst);
            return Err(self.locked_vault_message());
        };

        let hash = crate::data::vault_key::master_password_hash(&master);
        let Ok(decrypted) = crypto::decrypt(data, &hash) else {
            // O `.key` abriu **mas não é a chave deste vault**. Dois caminhos
            // chegam aqui, e a mensagem tem que servir aos dois:
            //  - `.key` órfão de um `set_password` interrompido → o vault é de
            //    senha e basta digitá-la;
            //  - vault cifrado por **outra** chave de aparelho (o usuário copiou um
            //    `AccountData.json` antigo por cima, ou restaurou um vault de um zip
            //    sem a chave dele) → nunca houve senha, e pedir senha e parar é um
            //    beco sem saída.
            // Nada é gravado nos dois casos (o latch abaixo garante).
            self.load_failed
                .store(true, std::sync::atomic::Ordering::SeqCst);
            // Os passos vão à mão: isto sai na tela de senha, onde o Settings
            // não abre.
            let (data_dir, backups_dir) = self.data_and_backups_dirs();
            return Err(format!(
                "Password required for encrypted file. If you never set a password, this file was \
                 encrypted with a different device key than {}: close the app, move AccountData.json \
                 and AccountData.key out of {} (keep them), put back both from a backup zip in {} \
                 (they have to come from the same zip), and open the app again. Nothing was deleted \
                 or overwritten.",
                self.key_file_path().display(),
                data_dir.display(),
                backups_dir.display()
            ));
        };

        let session = SessionKey::from_master_key(master)?;
        let mut slot = self.session.lock().map_err(|e| e.to_string())?;
        *slot = Some(session);
        drop(slot);

        self.commit_loaded(decrypted)
    }

    fn commit_loaded(&self, decrypted: Vec<u8>) -> Result<(), String> {
        let accounts = match Self::parse_accounts_json(&decrypted) {
            Ok(accounts) => accounts,
            Err(e) => {
                self.load_failed
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                return Err(e);
            }
        };
        let mut store = self.accounts.lock().map_err(|e| e.to_string())?;
        *store = accounts;
        drop(store);
        self.mark_memory_fresh();
        Ok(())
    }

    /// A senha abre o arquivo do disco? **Só confere**: não troca a sessão, não
    /// relê as contas para a memória, não grava nada. É o que destranca a tela
    /// de "trancado por inatividade" (ideia 27) sem mexer no que está rodando
    /// por baixo (AFK, reconexão, Auto Rejoin continuam com a sessão de agora).
    ///
    /// `Err` quando não há senha do usuário para conferir — trancar sem senha
    /// não faz sentido, e a UI nem liga o recurso nesse caso.
    pub fn verify_password(&self, password: &str) -> Result<bool, String> {
        if !self.has_user_password()? {
            return Err("No app password is set.".to_string());
        }
        let data =
            fs::read(&self.file_path).map_err(|e| format!("Failed to read account file: {}", e))?;
        let hash = crypto::hash_password(password.trim());
        Ok(crypto::decrypt(&data, &hash).is_ok())
    }

    pub fn load_with_password(&self, password: &str) -> Result<(), String> {
        // O unlock é o único ponto que paga o argon2: a chave derivada aqui é
        // reutilizada por todas as gravações da sessão.
        let trimmed = password.trim();
        let hash = crypto::hash_password(trimmed);

        if !self.file_path.exists() {
            let session = SessionKey::derive(trimmed)?;
            let mut slot = self.session.lock().map_err(|e| e.to_string())?;
            *slot = Some(session);
            return Ok(());
        }

        let data =
            fs::read(&self.file_path).map_err(|e| format!("Failed to read account file: {}", e))?;

        if data.is_empty() {
            let session = SessionKey::derive(trimmed)?;
            let mut accounts = self.accounts.lock().map_err(|e| e.to_string())?;
            *accounts = Vec::new();
            drop(accounts);
            let mut slot = self.session.lock().map_err(|e| e.to_string())?;
            *slot = Some(session);
            return Ok(());
        }

        let was_encrypted = crypto::is_encrypted(&data);
        let accounts = if was_encrypted {
            let decrypted = crypto::decrypt(&data, &hash).map_err(|_| {
                // Senha errada é o caso comum; vault cifrado pela chave de um
                // aparelho que não existe mais é o caso raro e grave, e pedir a
                // senha de novo não resolve. A mensagem tem que separar os dois.
                if self.key_file_path().exists() {
                    self.locked_vault_message()
                } else {
                    // Sem `.key` os dois motivos são indistinguíveis pelos
                    // arquivos: senha errada, ou vault **sem** senha cujo `.key`
                    // foi apagado (usuário, antivírus, limpeza de disco). O
                    // segundo não se resolve digitando de novo, e quem não souber
                    // disso vai ficar tentando senhas até desistir.
                    // E "restaure de um backup" sem dizer como é beco sem saída: esta
                    // mensagem sai na tela de senha, onde o Settings não abre.
                    let (data_dir, backups_dir) = self.data_and_backups_dirs();
                    format!(
                        "Failed to decrypt: wrong password. If this vault never had a password, \
                         its device key file is missing ({}) and retyping will not help: close the \
                         app, move AccountData.json out of {} (keep it), put back AccountData.json \
                         and AccountData.key from a backup zip in {}, and open the app again. A \
                         backup only opens on the PC and Windows user that made it.",
                        self.key_file_path().display(),
                        data_dir.display(),
                        backups_dir.display()
                    )
                }
            })?;
            Self::parse_accounts_json(&decrypted)?
        } else {
            Self::decode_plain_or_legacy_accounts(&data)?
        };

        // Derivado só depois da senha ser aceita, para uma senha errada não
        // custar um argon2 extra.
        let session = SessionKey::derive(trimmed)?;

        let mut store = self.accounts.lock().map_err(|e| e.to_string())?;
        *store = accounts;
        drop(store);
        self.load_failed
            .store(false, std::sync::atomic::Ordering::SeqCst);

        let mut slot = self.session.lock().map_err(|e| e.to_string())?;
        *slot = Some(session);
        drop(slot);

        if was_encrypted {
            // A senha abriu o arquivo, então este vault é de senha — e qualquer
            // `.key` ao lado dele é órfão (sobra de um `set_password` que morreu
            // entre gravar o vault e apagar a chave). Sem isto ele fica para
            // sempre, e todo boot acusa "a chave deste aparelho não pôde ser
            // recuperada" para quem só precisa digitar a senha. A senha é a
            // autoridade, igual em `set_password(Some)`.
            crate::data::vault_key::remove_key_file(&self.key_file_path());
        }
        Ok(())
    }

    pub fn save(&self) -> Result<(), String> {
        let accounts = self.accounts.lock().map_err(|e| e.to_string())?;
        self.save_locked(&accounts)
    }

    /// Serializa e grava o snapshot **que o chamador ainda está segurando**.
    ///
    /// `add`/`remove`/`update`/`reorder` soltavam o lock antes de `save()`
    /// reobtê-lo, então duas escritas concorrentes podiam intercalar e deixar o
    /// arquivo uma entrada atrás da memória. Mantendo o guard vivo até o
    /// `atomic_replace`, o que vai para o disco é sempre o estado que acabou de
    /// ser produzido.
    ///
    /// Sem segredo em memória a gravação só é permitida quando o arquivo **não**
    /// está cifrado: é o caminho degradado de quem não conseguiu criar a chave do
    /// aparelho (ver `load`). Com o arquivo cifrado e nada em memória, gravar
    /// seria trocar as contas do usuário por uma lista vazia.
    fn save_locked(&self, accounts: &[Account]) -> Result<(), String> {
        if self.load_failed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(
                "Account file could not be loaded; refusing to overwrite it. Fix or restore AccountData.json and restart.".to_string(),
            );
        }
        {
            let block = self.write_block.lock().map_err(|e| e.to_string())?;
            if let Some(reason) = block.as_deref() {
                return Err(format!(
                    "Refusing to write the account file: {}. Restart the app before changing accounts.",
                    reason
                ));
            }
        }

        let json = serde_json::to_string_pretty(accounts)
            .map_err(|e| format!("Failed to serialize accounts: {}", e))?;

        let session = self.session.lock().map_err(|e| e.to_string())?;

        // O `.key` pode ter desaparecido **ou ficado ilegível** com o app rodando
        // (antivírus, limpeza de disco, queda de energia no meio da regravação).
        // Sem isto, o app seguiria gravando o dia todo um vault que ninguém mais
        // abre — e cada backup automático criado depois levaria o vault **sem** a
        // chave dele, até a poda apagar os backups bons. É por isso que a
        // `SessionKey` guarda a chave mestra, não só o hash dela.
        //
        // A condição é "o `.key` guarda **a chave desta sessão**". Dois defeitos
        // reais moldaram isso:
        //  - "não existe" era o primeiro, e deixava passar o arquivo truncado;
        //  - "abre" deixava passar o `.key` que abre para **outra** chave — o caso
        //    da pasta de dados em OneDrive/Dropbox, em que o `.key` volta a uma
        //    versão anterior enquanto o vault fica novo. O vault seguia sendo
        //    gravado com o master da sessão e o boot seguinte pedia uma senha que
        //    nunca existiu.
        //
        // Qualquer outro estado **tenta** o reparo, inclusive "não consegui ler
        // agora": ler falhar não quer dizer que escrever vai falhar, e pular
        // deixaria um arquivo ruim sem reparo enquanto o processo vivesse. Quem
        // separa o transitório do grave é o **tom do aviso** (`is_transient_io`,
        // medido), não o pular.
        //
        // No caminho saudável isto custa uma leitura e uma chamada de DPAPI (sem
        // argon2); só o caminho quebrado paga caro, e paga uma vez.
        if let Some(session) = session.as_ref() {
            if let Some(master) = session.master.as_ref() {
                if crate::data::vault_key::key_file_holds_master(&self.key_file_path(), master) {
                    // Um aviso **sobre o `.key`** que tenha sobrado de uma falha
                    // anterior sai da tela. Com escopo: sem ele, isto apagava
                    // também o aviso de que o `AccountData.json` continua em texto
                    // puro, e apagava antes de a gravação abaixo dar certo.
                    self.clear_key_warning_for(&self.key_file_path());
                } else {
                    self.refresh_key_file(master);
                }
            }
        }

        let wrote_encrypted = session.is_some();
        let data = if let Some(session) = session.as_ref() {
            session.encrypt(&json)?
        } else {
            // Still locked: never replace an encrypted file with plaintext (it
            // would drop every account the user has not unlocked yet).
            if self.is_encrypted()? {
                return Err("Accounts are locked; unlock them before making changes.".to_string());
            }
            json.into_bytes()
        };

        // Write-then-fsync-then-rename: sem o fsync, o rename pode publicar um
        // arquivo cujo conteúdo ainda está em cache, e a queda de energia entre os
        // dois deixaria um `AccountData.json` que existe e está vazio.
        let tmp_path = self.file_path.with_extension("json.tmp");
        let synced = crate::data::versions::write_all_synced(&tmp_path, &data)
            .map_err(|e| format!("Failed to write account file: {}", e))?;
        crate::data::versions::atomic_replace(&tmp_path, &self.file_path)
            .map_err(|e| format!("Failed to replace account file: {}", e))?;

        // **Só depois de o arquivo estar no lugar.** Um aviso sobre o vault (o
        // "continua em texto puro" da migração) só pode ser considerado resolvido
        // por uma gravação que (a) deu certo e (b) foi **cifrada** — no caminho
        // degradado o arquivo continua legível, e limpar ali seria mentir.
        if wrote_encrypted {
            self.clear_key_warning_for(&self.file_path);
        }
        if !synced {
            // A gravação vale (os bytes foram entregues ao SO); o que se perde é a
            // garantia contra queda de energia. Descartar este `bool` tornava a
            // degradação 100% invisível. `set_key_warning` não deixa isto rebaixar
            // um aviso grave nem repetir a mesma linha a cada gravação.
            self.set_key_warning(VaultKeyWarning::sync_unconfirmed(&self.file_path));
        }

        Ok(())
    }

    pub fn set_password(&self, password: Option<&str>) -> Result<(), String> {
        // Antes de qualquer validação que mexa em arquivo: com a gravação trancada
        // (restauração de backup) isto não pode nem começar.
        self.ensure_writable()?;

        if let Some(value) = password {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err("Password cannot be empty".to_string());
            }
            if trimmed.chars().count() < 8 {
                return Err("Password must be at least 8 characters".to_string());
            }
        }
        let key_path = self.key_file_path();

        {
            let slot = self.session.lock().map_err(|e| e.to_string())?;
            if slot.is_none() && self.is_encrypted()? {
                // Re-keying a file we never decrypted would encrypt an empty list
                // over the user's accounts.
                return Err(
                    "Accounts are locked; unlock them before changing the password.".to_string(),
                );
            }
        }

        // **Aqui não se faz cópia**, e isso é decisão, não esquecimento.
        //
        // A cópia que existia (`.json.rekey.bak`) era cifrada pela chave do
        // aparelho e o `.key` era apagado três linhas depois: um arquivo que nunca
        // mais abria, que ninguém limpava e que nada avisava — exatamente o
        // "arquivo com cara de backup que não abre com nada" que o comentário da
        // rodada anterior usou como argumento para renomeá-la. Renomear não
        // resolveu; eliminar resolve.
        //
        // O que protege esta operação não é uma cópia: é (a) a gravação atômica,
        // que deixa o arquivo antigo intacto em qualquer falha, e (b) a ordem
        // "chave antes do arquivo que ela cifra". A cópia em texto puro da
        // migração (`.json.bak`) continua sendo a rede de verdade, e continua
        // intocada aqui.
        //
        // Sobra de versão anterior do app sai de cena, para ninguém confiar nela.
        let _ = fs::remove_file(self.file_path.with_extension("json.rekey.bak"));

        match password {
            Some(value) => {
                // A sessão da senha só passa a valer **depois** de o vault estar
                // gravado com ela. Trocar antes (como era) fazia a senha valer
                // mesmo quando a gravação falhava: a UI dizia que não aplicou, e a
                // próxima gravação de fundo que desse certo cifrava com a senha —
                // com o `.key` ainda no disco, o boot seguinte mandava restaurar
                // uma chave em vez de pedir a senha.
                //
                // O argon2 roda antes do lock (custa caro). A troca, a gravação e
                // a volta no erro acontecem com `accounts` seguro, então nenhuma
                // gravação de fundo usa a sessão da senha antes de ela valer.
                // Ordem de lock do arquivo: accounts → session.
                let password_session = SessionKey::derive(value.trim())?;
                let accounts = self.accounts.lock().map_err(|e| e.to_string())?;
                let previous = self
                    .session
                    .lock()
                    .map_err(|e| e.to_string())?
                    .replace(password_session);
                if let Err(e) = self.save_locked(&accounts) {
                    *self
                        .session
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = previous;
                    return Err(e);
                }
                // O `.key` só sai **depois** de o vault estar gravado com a
                // senha. Na ordem contrária, uma falha na gravação deixaria um
                // vault cifrado pela chave do aparelho sem a chave para abri-lo.
                crate::data::vault_key::remove_key_file(&key_path);
                drop(accounts);
                // A partir daqui o `.key` não importa mais, então um aviso sobre
                // ele é falso alarme. Sem isto, quem estava com a faixa vermelha e
                // definia uma senha ficava com ela na tela o resto da sessão,
                // apontando para um arquivo que acabou de ser apagado — e nenhum
                // caminho de sessão-com-senha alcança o `clear_key_warning`.
                self.clear_key_warning();
                Ok(())
            }
            None => {
                // Tirar a senha volta para a chave do aparelho, **não** para
                // texto puro. A chave é montada (e o `.key` gravado) antes de a
                // sessão da senha ser trocada: erro aqui deixa o arquivo e a
                // sessão exatamente como estavam.
                let password_in_effect = self.has_user_password().unwrap_or(false);
                let key_existed = key_path.exists();
                let device_session = self.build_device_session().map_err(|e| {
                    if password_in_effect {
                        // O vault continua cifrado com a senha, e nada depende
                        // do `.key` que não saiu: o aviso sobre ele mandaria pôr
                        // senha em quem já tem, e o erro de `build_device_session`
                        // diria que o arquivo ficou sem cifra.
                        self.clear_key_warning_for(&key_path);
                        if !key_existed {
                            return format!(
                                "Removing the password failed: the account key file ({})                                  could not be created, so the password is still in use.",
                                key_path.display()
                            );
                        }
                        return e;
                    }
                    // Nova tentativa depois de uma migração ou primeiro boot que
                    // falharam (é o que a faixa manda fazer): sem sessão, o
                    // arquivo segue em texto puro, e a faixa não pode trocar isso
                    // pelo aviso do `.key` desta tentativa.
                    self.warn_left_in_plain_text(&e);
                    e
                })?;
                // Como no ramo da senha: a chave do aparelho só passa a valer
                // **depois** de o vault estar gravado com ela. Trocar antes fazia
                // a remoção valer mesmo quando a gravação falhava — a UI dizia
                // que a senha ficou, e a próxima gravação de fundo a tirava.
                // Ordem de lock do arquivo: accounts → session.
                let accounts = self.accounts.lock().map_err(|e| e.to_string())?;
                let previous = self
                    .session
                    .lock()
                    .map_err(|e| e.to_string())?
                    .replace(device_session);
                if let Err(e) = self.save_locked(&accounts) {
                    *self
                        .session
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = previous;
                    // O `.key` que esta tentativa criou não cifra nada.
                    if !key_existed {
                        crate::data::vault_key::remove_key_file(&key_path);
                    }
                    return Err(e);
                }
                Ok(())
            }
        }
    }

    pub fn get_all(&self) -> Result<Vec<Account>, String> {
        let accounts = self.accounts.lock().map_err(|e| e.to_string())?;
        Ok(accounts.clone())
    }

    pub fn add(&self, account: Account) -> Result<(), String> {
        let mut accounts = self.accounts.lock().map_err(|e| e.to_string())?;

        if let Some(existing) = accounts.iter_mut().find(|a| a.user_id == account.user_id) {
            existing.security_token = account.security_token;
            existing.username = account.username;
            existing.valid = account.valid;
            existing.last_use = account.last_use;
            if !account.password.is_empty() {
                existing.password = account.password;
            }
        } else {
            accounts.push(account);
        }

        // O guard continua vivo: o arquivo recebe exatamente este snapshot.
        self.save_locked(&accounts)
    }

    pub fn remove(&self, user_id: i64) -> Result<bool, String> {
        let mut accounts = self.accounts.lock().map_err(|e| e.to_string())?;
        let initial_len = accounts.len();
        accounts.retain(|a| a.user_id != user_id);
        let removed = accounts.len() < initial_len;

        if removed {
            self.save_locked(&accounts)?;
        }

        Ok(removed)
    }

    pub fn update(&self, account: Account) -> Result<bool, String> {
        let mut accounts = self.accounts.lock().map_err(|e| e.to_string())?;

        if let Some(existing) = accounts.iter_mut().find(|a| a.user_id == account.user_id) {
            *existing = account;
            self.save_locked(&accounts)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Troca o cookie da conta por `new_token` **só se** ela ainda estiver com
    /// `old_token` — tudo sob o mesmo lock, e gravado pelo caminho normal
    /// (criptografado). É o que grava o cookie novo que o Roblox devolveu numa
    /// resposta (`api::cookie_rotation`): se a conta já ganhou outro cookie
    /// nesse meio-tempo (novo login, refresh), o da resposta velha não passa
    /// por cima. Marca `valid = true`: o Roblox acabou de entregar sessão nova.
    pub fn replace_token_if(
        &self,
        user_id: i64,
        old_token: &str,
        new_token: &str,
    ) -> Result<bool, String> {
        let mut accounts = self.accounts.lock().map_err(|e| e.to_string())?;
        let Some(account) = accounts
            .iter_mut()
            .find(|a| a.user_id == user_id && a.security_token == old_token)
        else {
            return Ok(false);
        };
        account.security_token = new_token.to_string();
        account.valid = true;
        self.save_locked(&accounts)?;
        Ok(true)
    }

    /// Grava o `valid` da conta (o ponto vermelho de sessão inválida). Só
    /// regrava o arquivo quando o valor muda — conferir 50 contas boas não pode
    /// virar 50 gravações. Devolve `true` quando mudou.
    pub fn set_valid(&self, user_id: i64, valid: bool) -> Result<bool, String> {
        let mut accounts = self.accounts.lock().map_err(|e| e.to_string())?;
        let Some(account) = accounts.iter_mut().find(|a| a.user_id == user_id) else {
            return Ok(false);
        };
        if account.valid == valid {
            return Ok(false);
        }
        account.valid = valid;
        self.save_locked(&accounts)?;
        Ok(true)
    }

    /// Marca que a conta **foi usada agora**. Chamado no sucesso do launch (app,
    /// botting e web server): sem isso `last_use` só era escrito ao criar ou
    /// re-adicionar a conta, e a coluna "3d"/"2mo" da lista media idade do
    /// cadastro em vez de inatividade de jogo. Devolve `false` quando não existe
    /// conta com esse id — lançar uma conta que saiu da lista não é erro.
    pub fn mark_used(&self, user_id: i64) -> Result<bool, String> {
        let mut accounts = self.accounts.lock().map_err(|e| e.to_string())?;

        let Some(account) = accounts.iter_mut().find(|a| a.user_id == user_id) else {
            return Ok(false);
        };
        account.last_use = Utc::now();

        self.save_locked(&accounts)?;
        Ok(true)
    }

    pub fn reorder(&self, user_ids: &[i64]) -> Result<(), String> {
        let mut accounts = self.accounts.lock().map_err(|e| e.to_string())?;

        if accounts.is_empty() || user_ids.is_empty() {
            return Ok(());
        }

        let mut ordered = Vec::with_capacity(accounts.len());

        for user_id in user_ids {
            if let Some(pos) = accounts.iter().position(|a| a.user_id == *user_id) {
                ordered.push(accounts.remove(pos));
            }
        }

        ordered.append(&mut accounts);
        *accounts = ordered;

        self.save_locked(&accounts)
    }

    fn decode_plain_or_legacy_accounts(data: &[u8]) -> Result<Vec<Account>, String> {
        if let Ok(accounts) = Self::parse_accounts_json(data) {
            return Ok(accounts);
        }

        if let Some(legacy_decrypted) = crypto::try_decrypt_legacy_dpapi(data) {
            return Self::parse_accounts_json(&legacy_decrypted);
        }

        Err("Invalid account data format (failed plaintext and legacy DPAPI decode)".to_string())
    }

    fn decode_accounts_for_import(
        &self,
        data: &[u8],
        import_password: Option<&str>,
    ) -> Result<Vec<Account>, String> {
        if data.is_empty() {
            return Ok(Vec::new());
        }

        if crypto::is_encrypted(data) {
            if let Some(password) = import_password {
                // Mesmo trim de `load_with_password`: a senha colada com espaço no
                // fim desbloqueava o app mas era recusada no import.
                let hash = crypto::hash_password(password.trim());
                let decrypted = crypto::decrypt(data, &hash)
                    .map_err(|_| "Import password is incorrect".to_string())?;
                return Self::parse_accounts_json(&decrypted);
            }

            // Sem senha: o arquivo pode ser um backup **deste** vault, cifrado
            // pela chave do aparelho. Tentar os segredos que já temos resolve o
            // caso normal de "exportei e reimportei nesta máquina" sem pedir uma
            // senha que nunca existiu.
            for hash in self.own_secret_hashes()? {
                if let Ok(decrypted) = crypto::decrypt(data, &hash) {
                    return Self::parse_accounts_json(&decrypted);
                }
            }

            return Err(IMPORT_PASSWORD_REQUIRED.to_string());
        }

        Self::decode_plain_or_legacy_accounts(data)
    }

    /// Hashes que este app pode ter usado para cifrar um vault: o da sessão atual
    /// e o da chave mestra em disco. Nunca vaza a chave — só o hash derivado, que
    /// é o que `crypto::decrypt` consome.
    fn own_secret_hashes(&self) -> Result<Vec<Vec<u8>>, String> {
        let mut hashes: Vec<Vec<u8>> = Vec::new();
        {
            let session = self.session.lock().map_err(|e| e.to_string())?;
            if let Some(session) = session.as_ref() {
                hashes.push(session.password_hash.clone());
            }
        }
        if let Some(recovered) = crate::data::vault_key::load_master_key(&self.key_file_path()) {
            let hash = crate::data::vault_key::master_password_hash(&recovered.master);
            if !hashes.contains(&hash) {
                hashes.push(hash);
            }
        }
        Ok(hashes)
    }

    fn parse_accounts_json(data: &[u8]) -> Result<Vec<Account>, String> {
        serde_json::from_slice::<Vec<Account>>(data)
            .map_err(|e| format!("Failed to parse account JSON: {}", e))
    }

    pub fn import_old_account_data(
        &self,
        data: &[u8],
        import_password: Option<&str>,
    ) -> Result<OldAccountImportSummary, String> {
        let imported_accounts = self.decode_accounts_for_import(data, import_password)?;
        let total = imported_accounts.len();
        let mut skipped = 0usize;

        let mut imported_by_user_id: HashMap<i64, Account> = HashMap::new();
        let mut imported_order: Vec<i64> = Vec::new();

        for account in imported_accounts {
            let user_id = account.user_id;
            if user_id <= 0 {
                skipped += 1;
                continue;
            }

            if imported_by_user_id.contains_key(&user_id) {
                skipped += 1;
            } else {
                imported_order.push(user_id);
            }
            imported_by_user_id.insert(user_id, account);
        }

        let mut accounts = self.accounts.lock().map_err(|e| e.to_string())?;
        let mut current_index_by_user_id: HashMap<i64, usize> = accounts
            .iter()
            .enumerate()
            .map(|(idx, account)| (account.user_id, idx))
            .collect();

        let mut added = 0usize;
        let mut replaced = 0usize;

        for user_id in imported_order {
            let Some(account) = imported_by_user_id.remove(&user_id) else {
                continue;
            };
            if let Some(existing_index) = current_index_by_user_id.get(&user_id).copied() {
                accounts[existing_index] = account;
                replaced += 1;
            } else {
                let next_index = accounts.len();
                current_index_by_user_id.insert(user_id, next_index);
                accounts.push(account);
                added += 1;
            }
        }

        let mut seen_user_ids = HashSet::new();
        accounts.retain(|account| seen_user_ids.insert(account.user_id));

        if added > 0 || replaced > 0 {
            self.save_locked(&accounts)?;
        }
        drop(accounts);

        Ok(OldAccountImportSummary {
            total,
            added,
            replaced,
            skipped,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("ram-{name}-{nanos}.json"))
    }

    fn new_test_store(name: &str) -> AccountStore {
        crypto::init();
        AccountStore::new(unique_test_path(name))
    }

    #[test]
    fn decode_plain_or_legacy_accounts_should_accept_legacy_null_string_fields() {
        let json = br#"
        [
          {
            "Valid": true,
            "SecurityToken": "_|WARNING:-DO-NOT-SHARE",
            "Username": "LegacyUser",
            "LastUse": "2024-03-05T12:34:56",
            "Alias": null,
            "Description": null,
            "Password": null,
            "Group": null,
            "UserID": 12345,
            "Fields": { "Note": null, "Rank": "Admin" },
            "LastAttemptedRefresh": "2024-03-05T12:34:56",
            "BrowserTrackerID": null
          }
        ]
        "#;

        let accounts = AccountStore::decode_plain_or_legacy_accounts(json).unwrap();

        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].alias, "");
        assert_eq!(accounts[0].description, "");
        assert_eq!(accounts[0].password, "");
        assert_eq!(accounts[0].group, "Default");
        assert_eq!(accounts[0].browser_tracker_id, "");
        assert_eq!(accounts[0].fields.get("Note").map(String::as_str), Some(""));
        assert_eq!(
            accounts[0].fields.get("Rank").map(String::as_str),
            Some("Admin")
        );
    }

    #[test]
    fn import_old_account_data_should_accept_current_v4_encrypted_exports() {
        let store = new_test_store("import-current-v4");
        let current = vec![Account::new(
            "_|WARNING:-DO-NOT-SHARE".to_string(),
            "CurrentUser".to_string(),
            67890,
        )];
        let json = serde_json::to_string(&current).unwrap();
        let password = "compatibility-pass";
        let hash = crypto::hash_password(password);
        let encrypted = crypto::encrypt(&json, &hash).unwrap();

        let summary = store
            .import_old_account_data(&encrypted, Some(password))
            .unwrap();
        let imported = store.get_all().unwrap();

        assert_eq!(summary.total, 1);
        assert_eq!(summary.added, 1);
        assert_eq!(summary.replaced, 0);
        assert_eq!(summary.skipped, 0);
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].user_id, 67890);
        assert_eq!(imported[0].username, "CurrentUser");

        let _ = fs::remove_file(&store.file_path);
    }

    #[test]
    fn save_should_not_overwrite_encrypted_file_while_locked() {
        let store = new_test_store("locked-save");
        let existing = vec![Account::new("cookie".to_string(), "Kept".to_string(), 111)];
        let json = serde_json::to_string(&existing).unwrap();
        let encrypted = crypto::encrypt(&json, &crypto::hash_password("secret-pass")).unwrap();
        fs::write(&store.file_path, &encrypted).unwrap();

        let result = store.add(Account::new("c2".to_string(), "New".to_string(), 222));

        assert!(result.is_err());
        assert_eq!(fs::read(&store.file_path).unwrap(), encrypted);
        let _ = fs::remove_file(&store.file_path);
    }

    /// Tirar a senha **não** decifra mais o arquivo: ele passa a ser cifrado pela
    /// chave do aparelho. Antes desta mudança o mesmo clique deixava os cookies
    /// de todas as contas legíveis em disco.
    #[test]
    fn set_password_none_rekeys_an_unlocked_store_to_the_device_key() {
        let store = new_test_store("remove-encryption");
        let existing = vec![Account::new("cookie".to_string(), "Kept".to_string(), 111)];
        let json = serde_json::to_string(&existing).unwrap();
        let encrypted = crypto::encrypt(&json, &crypto::hash_password("secret-pass")).unwrap();
        fs::write(&store.file_path, &encrypted).unwrap();
        store.load_with_password("secret-pass").unwrap();

        store.set_password(None).unwrap();

        assert!(
            store.is_encrypted().unwrap(),
            "o arquivo tem que continuar cifrado"
        );
        assert!(
            !store.has_user_password().unwrap(),
            "não há mais senha de usuário"
        );
        assert_eq!(store.get_all().unwrap()[0].user_id, 111);
        let _ = fs::remove_file(&store.file_path);
        let _ = fs::remove_file(store.file_path.with_extension("key"));
        let _ = fs::remove_file(store.file_path.with_extension("json.bak"));
    }

    #[test]
    fn save_should_refuse_after_failed_load() {
        let store = new_test_store("failed-load");
        fs::write(&store.file_path, b"not valid account data").unwrap();

        assert!(store.load().is_err());
        assert!(store.add(Account::new("c".to_string(), "U".to_string(), 1)).is_err());
        assert_eq!(fs::read(&store.file_path).unwrap(), b"not valid account data");
        let _ = fs::remove_file(&store.file_path);
    }

    #[test]
    fn save_should_write_atomically_and_round_trip() {
        let store = new_test_store("atomic-save");
        store.add(Account::new("c".to_string(), "U".to_string(), 7)).unwrap();

        assert!(!store.file_path.with_extension("json.tmp").exists());
        let reloaded = AccountStore::new(store.file_path.clone());
        reloaded.load().unwrap();
        assert_eq!(reloaded.get_all().unwrap()[0].user_id, 7);
        let _ = fs::remove_file(&store.file_path);
    }

    #[test]
    fn import_old_account_data_should_accept_current_v4_plain_exports() {
        let store = new_test_store("import-current-v4-plain");
        let current = vec![Account::new(
            "_|WARNING:-DO-NOT-SHARE".to_string(),
            "PlainUser".to_string(),
            24680,
        )];
        let json = serde_json::to_vec(&current).unwrap();

        let summary = store.import_old_account_data(&json, None).unwrap();
        let imported = store.get_all().unwrap();

        assert_eq!(summary.total, 1);
        assert_eq!(summary.added, 1);
        assert_eq!(summary.replaced, 0);
        assert_eq!(summary.skipped, 0);
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].user_id, 24680);
        assert_eq!(imported[0].username, "PlainUser");

        let _ = fs::remove_file(&store.file_path);
    }
}

#[cfg(test)]
mod account_store_tests {
    use super::*;
    use std::sync::OnceLock;
    use std::time::{SystemTime, UNIX_EPOCH};

    const SAMPLE_PASSWORD: &str = "sample-password";

    fn temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("ram-acct-{name}-{nanos}.json"))
    }

    /// Store + limpeza RAII do arquivo e de **tudo** que o vault pode deixar ao
    /// lado dele. Desde a criptografia por padrão isso inclui o `.key` e o
    /// `.json.bak`; sem eles na lista, cada execução deixava lixo no `%TEMP%`.
    struct TestStore {
        store: AccountStore,
    }

    impl Drop for TestStore {
        fn drop(&mut self) {
            let path = self.store.file_path.clone();
            for extra in [
                "json.tmp",
                "json.bak",
                "json.rekey.bak",
                "key",
                "key.tmp",
                "key.probe",
            ] {
                let _ = fs::remove_file(path.with_extension(extra));
            }
            let _ = fs::remove_file(&path);
        }
    }

    impl std::ops::Deref for TestStore {
        type Target = AccountStore;
        fn deref(&self) -> &AccountStore {
            &self.store
        }
    }

    fn store(name: &str) -> TestStore {
        crypto::init();
        TestStore {
            store: AccountStore::new(temp_path(name)),
        }
    }

    /// `LastUse` era escrito so na criacao/re-adicao da conta: a coluna "3d"/"2mo"
    /// e a bolinha de envelhecimento mediam idade do **cadastro**, nao inatividade
    /// de jogo. Quem lanca precisa poder marcar uso.
    #[test]
    fn mark_used_moves_last_use_forward_and_persists_it() {
        let store = store("mark-used");
        let mut old = account(7, "ann");
        old.last_use = chrono::Utc::now() - chrono::Duration::days(40);
        let before = old.last_use;
        store.add(old).unwrap();

        assert!(store.mark_used(7).unwrap(), "a conta existe, entao marcou");

        let after = store.get_all().unwrap()[0].last_use;
        assert!(after > before, "last_use andou para frente: {before} -> {after}");

        // E foi para o disco, nao so para a memoria.
        let reloaded = AccountStore::new(store.file_path.clone());
        reloaded.load().unwrap();
        assert_eq!(reloaded.get_all().unwrap()[0].last_use, after);
    }

    #[test]
    fn mark_used_says_when_the_account_is_not_there() {
        let store = store("mark-used-missing");
        store.add(account(7, "ann")).unwrap();
        assert!(!store.mark_used(999).unwrap(), "conta inexistente nao marca nada");
    }

    fn account(user_id: i64, username: &str) -> Account {
        Account::new(format!("cookie-{user_id}"), username.to_string(), user_id)
    }

    /// Deriving an argon2i key is deliberately slow, so the tests that only
    /// need "some encrypted file" share one blob.
    fn encrypted_sample() -> &'static Vec<u8> {
        static SAMPLE: OnceLock<Vec<u8>> = OnceLock::new();
        SAMPLE.get_or_init(|| {
            crypto::init();
            let json = serde_json::to_string(&vec![account(111, "Sample")]).unwrap();
            crypto::encrypt(&json, &crypto::hash_password(SAMPLE_PASSWORD)).unwrap()
        })
    }

    fn ids(store: &AccountStore) -> Vec<i64> {
        store.get_all().unwrap().iter().map(|a| a.user_id).collect()
    }

    // ---- is_encrypted / needs_password ---------------------------------------

    #[test]
    fn is_encrypted_and_needs_password_follow_the_file_on_disk() {
        let s = store("encstate");

        // No file yet: nothing is encrypted and nothing is locked.
        assert!(!s.is_encrypted().unwrap());
        assert!(!s.needs_password().unwrap());

        // Plain file: still unlocked.
        fs::write(&s.file_path, b"[]").unwrap();
        assert!(!s.is_encrypted().unwrap());
        assert!(!s.needs_password().unwrap());

        // Empty file: `is_encrypted` looks at the bytes, so it is not encrypted.
        fs::write(&s.file_path, b"").unwrap();
        assert!(!s.is_encrypted().unwrap());
        assert!(!s.needs_password().unwrap());

        // Encrypted file with no password in memory: locked.
        fs::write(&s.file_path, encrypted_sample()).unwrap();
        assert!(s.is_encrypted().unwrap());
        assert!(s.needs_password().unwrap());

        // Once a password is held, it is no longer "needs password" even
        // though the file is still encrypted.
        *s.session.lock().unwrap() = Some(SessionKey::derive(SAMPLE_PASSWORD).unwrap());
        assert!(s.is_encrypted().unwrap());
        assert!(!s.needs_password().unwrap());
    }

    // ---- load ----------------------------------------------------------------

    #[test]
    fn load_is_a_no_op_for_a_missing_or_empty_file() {
        let s = store("load-empty");
        s.load().expect("missing file must not be an error");
        assert!(ids(&s).is_empty());
        assert!(!s.file_path.exists(), "load must not create the file");

        fs::write(&s.file_path, b"").unwrap();
        s.load().expect("empty file must not be an error");
        assert!(ids(&s).is_empty());
        // An empty file is not a failed load, so saving stays allowed.
        s.add(account(1, "A")).expect("save after empty load");
        assert_eq!(ids(&s), vec![1]);
    }

    #[test]
    fn load_reads_plain_json_and_replaces_the_in_memory_list() {
        let s = store("load-plain");
        s.add(account(9, "Stale")).unwrap();

        let json = serde_json::to_vec(&vec![account(1, "One"), account(2, "Two")]).unwrap();
        fs::write(&s.file_path, json).unwrap();
        s.load().unwrap();

        assert_eq!(ids(&s), vec![1, 2], "load replaces, it does not merge");
    }

    #[test]
    fn load_of_an_encrypted_file_without_a_password_fails_and_latches_load_failed() {
        let s = store("load-locked");
        fs::write(&s.file_path, encrypted_sample()).unwrap();

        let err = s.load().expect_err("locked file must not load");
        assert!(err.contains("Password required"), "{err}");
        assert!(s.load_failed.load(std::sync::atomic::Ordering::SeqCst));

        // The latch must keep save() from clobbering the file.
        let err = s.save().expect_err("save must refuse after a failed load");
        assert!(err.contains("refusing to overwrite"), "{err}");
        assert_eq!(&fs::read(&s.file_path).unwrap(), encrypted_sample());
    }

    #[test]
    fn load_of_unreadable_bytes_fails_and_keeps_the_file_intact() {
        let s = store("load-garbage");
        fs::write(&s.file_path, b"\x00\x01\x02 definitely not json").unwrap();

        let err = s.load().expect_err("garbage must not load");
        assert!(
            err.contains("failed plaintext and legacy DPAPI decode"),
            "{err}"
        );
        assert!(
            s.add(account(1, "New")).is_err(),
            "writes must go through the save latch"
        );
        assert_eq!(
            fs::read(&s.file_path).unwrap(),
            b"\x00\x01\x02 definitely not json"
        );
    }

    #[test]
    fn a_successful_load_clears_a_previous_load_failure() {
        let s = store("load-recover");
        fs::write(&s.file_path, b"broken").unwrap();
        assert!(s.load().is_err());
        assert!(s.load_failed.load(std::sync::atomic::Ordering::SeqCst));

        fs::write(&s.file_path, b"[]").unwrap();
        s.load().expect("a good file must clear the latch");
        assert!(!s.load_failed.load(std::sync::atomic::Ordering::SeqCst));
        s.add(account(5, "Ok")).expect("saving is allowed again");
    }

    // ---- load_with_password ---------------------------------------------------

    #[test]
    fn load_with_password_on_a_missing_file_only_arms_the_password() {
        let s = store("pw-missing");
        s.load_with_password("  some-password  ").unwrap();

        assert!(!s.file_path.exists(), "no file must be created");
        assert!(ids(&s).is_empty());
        // The password is trimmed before hashing.
        assert_eq!(
            s.session
                .lock()
                .unwrap()
                .as_ref()
                .map(|k| k.password_hash.clone()),
            Some(crypto::hash_password("some-password"))
        );
    }

    #[test]
    fn load_with_password_on_an_empty_file_clears_accounts_and_arms_the_password() {
        let s = store("pw-empty");
        s.add(account(7, "Stale")).unwrap();
        fs::write(&s.file_path, b"").unwrap();

        s.load_with_password("another-password").unwrap();

        assert!(ids(&s).is_empty());
        assert!(s.session.lock().unwrap().is_some());
    }

    #[test]
    fn load_with_password_reads_a_plain_file_and_upgrades_it_on_the_next_save() {
        let s = store("pw-plain");
        let json = serde_json::to_vec(&vec![account(3, "Plain")]).unwrap();
        fs::write(&s.file_path, json).unwrap();

        s.load_with_password("upgrade-password").unwrap();
        assert_eq!(ids(&s), vec![3]);

        // Holding a password makes every later save encrypt.
        s.add(account(4, "New")).unwrap();
        assert!(s.is_encrypted().unwrap());
    }

    #[test]
    fn load_with_password_round_trips_an_encrypted_file_and_rejects_the_wrong_password() {
        let s = store("pw-roundtrip");
        fs::write(&s.file_path, encrypted_sample()).unwrap();

        let err = s
            .load_with_password("not-the-password")
            .expect_err("wrong password must fail");
        assert!(err.contains("Failed to decrypt"), "{err}");
        assert!(ids(&s).is_empty(), "a failed unlock must not load anything");
        assert!(
            s.session.lock().unwrap().is_none(),
            "a failed unlock must not arm the wrong password"
        );

        s.load_with_password(SAMPLE_PASSWORD).unwrap();
        assert_eq!(ids(&s), vec![111]);
        assert_eq!(s.get_all().unwrap()[0].username, "Sample");
    }

    // ---- save -----------------------------------------------------------------

    #[test]
    fn save_leaves_no_temp_file_and_round_trips_through_a_fresh_store() {
        let s = store("save-roundtrip");
        let mut a = account(21, "Persisted");
        a.alias = "Alt 21".to_string();
        a.group = "Farm".to_string();
        a.set_field("Note".into(), "keep me".into());
        s.add(a).unwrap();

        assert!(!s.file_path.with_extension("json.tmp").exists());

        let reloaded = AccountStore::new(s.file_path.clone());
        reloaded.load().unwrap();
        let loaded = reloaded.get_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].alias, "Alt 21");
        assert_eq!(loaded[0].group, "Farm");
        assert_eq!(loaded[0].get_field("Note").map(String::as_str), Some("keep me"));
    }

    /// A pré-condição real é **"nenhuma sessão foi estabelecida"** — o helper
    /// `store()` não chama `load()`, então não há segredo em memória e o arquivo
    /// não está cifrado. No app entregue, "sem senha" é justamente o caso
    /// **cifrado** (chave do aparelho), então o nome antigo
    /// (`save_writes_plain_json_when_no_password_is_set`) afirmava o contrário do
    /// comportamento que a Task 8 criou. Este é o caminho degradado, e só ele.
    #[test]
    fn save_writes_plain_json_only_in_the_degraded_no_key_path() {
        let s = store("save-plain");
        s.add(account(31, "Plain")).unwrap();

        let raw = fs::read_to_string(&s.file_path).unwrap();
        assert!(raw.trim_start().starts_with('['), "{raw}");
        assert!(raw.contains("\"UserID\": 31"), "{raw}");
        assert!(!s.is_encrypted().unwrap());
    }

    // ---- set_password ---------------------------------------------------------

    #[test]
    fn set_password_validates_the_new_password_before_touching_anything() {
        let s = store("pw-validate");
        s.add(account(41, "A")).unwrap();
        let before = fs::read(&s.file_path).unwrap();

        assert_eq!(
            s.set_password(Some("")).unwrap_err(),
            "Password cannot be empty"
        );
        assert_eq!(
            s.set_password(Some("    ")).unwrap_err(),
            "Password cannot be empty"
        );
        assert_eq!(
            s.set_password(Some("1234567")).unwrap_err(),
            "Password must be at least 8 characters"
        );
        // The length rule counts characters, not bytes, and applies after trim.
        assert_eq!(
            s.set_password(Some("  1234567  ")).unwrap_err(),
            "Password must be at least 8 characters"
        );
        assert!(
            s.set_password(Some("aaaaaaa\u{00e7}")).is_ok(),
            "8 characters must be accepted even when they are 9 bytes"
        );

        assert_ne!(fs::read(&s.file_path).unwrap(), before, "the file is re-keyed");
        assert!(s.is_encrypted().unwrap());
    }

    #[test]
    fn set_password_refuses_to_rekey_a_file_that_was_never_unlocked() {
        let s = store("pw-rekey-locked");
        fs::write(&s.file_path, encrypted_sample()).unwrap();

        let err = s
            .set_password(Some("brand-new-password"))
            .expect_err("re-keying a locked file must fail");
        assert!(err.contains("unlock them before changing the password"), "{err}");
        assert_eq!(&fs::read(&s.file_path).unwrap(), encrypted_sample());

        // Clearing the password on a locked store is refused for the same reason.
        let err = s.set_password(None).expect_err("clearing must fail too");
        assert!(err.contains("unlock them before changing the password"), "{err}");
        assert_eq!(&fs::read(&s.file_path).unwrap(), encrypted_sample());
    }

    /// "Sem senha" virou "com a chave do aparelho": pedir para tirar a senha de
    /// um store que não tem senha ainda tem que deixar o arquivo **cifrado**.
    #[test]
    fn set_password_none_on_a_store_without_a_password_encrypts_with_the_device_key() {
        let s = store("pw-none-plain");
        s.load().unwrap();
        s.add(account(51, "A")).unwrap();
        s.set_password(None).unwrap();
        assert!(s.is_encrypted().unwrap());
        assert!(!s.has_user_password().unwrap());
        assert_eq!(ids(&s), vec![51]);
    }

    // ---- add ------------------------------------------------------------------

    #[test]
    fn add_appends_new_accounts_in_order() {
        let s = store("add-order");
        s.add(account(1, "One")).unwrap();
        s.add(account(2, "Two")).unwrap();
        s.add(account(3, "Three")).unwrap();
        assert_eq!(ids(&s), vec![1, 2, 3]);
    }

    #[test]
    fn add_of_an_existing_user_id_refreshes_credentials_but_keeps_user_metadata() {
        let s = store("add-merge");
        let mut original = account(60, "OldName");
        original.alias = "My Alt".to_string();
        original.group = "Farm".to_string();
        original.description = "notes".to_string();
        original.password = "old-password".to_string();
        original.set_field("Note".into(), "kept".into());
        original.valid = false;
        s.add(original).unwrap();

        let mut incoming = account(60, "NewName");
        incoming.security_token = "fresh-cookie".to_string();
        incoming.valid = true;
        incoming.password = String::new(); // empty => must not clear the stored one
        s.add(incoming).unwrap();

        let stored = s.get_all().unwrap();
        assert_eq!(stored.len(), 1, "add must not duplicate a user id");
        assert_eq!(stored[0].username, "NewName");
        assert_eq!(stored[0].security_token, "fresh-cookie");
        assert!(stored[0].valid);
        assert_eq!(stored[0].password, "old-password", "an empty password must not wipe");
        assert_eq!(stored[0].alias, "My Alt");
        assert_eq!(stored[0].group, "Farm");
        assert_eq!(stored[0].description, "notes");
        assert_eq!(stored[0].get_field("Note").map(String::as_str), Some("kept"));

        // A non-empty password does replace it.
        let mut with_password = account(60, "NewName");
        with_password.password = "new-password".to_string();
        s.add(with_password).unwrap();
        assert_eq!(s.get_all().unwrap()[0].password, "new-password");
    }

    #[test]
    fn add_accepts_the_zero_user_id_as_a_normal_slot() {
        // `add` has no user-id validation: the guard only exists on import.
        let s = store("add-zero");
        s.add(account(0, "Zero")).unwrap();
        s.add(account(0, "ZeroAgain")).unwrap();
        assert_eq!(ids(&s), vec![0]);
        assert_eq!(s.get_all().unwrap()[0].username, "ZeroAgain");
    }

    // ---- remove ---------------------------------------------------------------

    #[test]
    fn remove_reports_whether_anything_was_removed_and_only_saves_when_it_was() {
        let s = store("remove");
        assert!(!s.remove(1).unwrap(), "removing from an empty store is false");
        assert!(!s.file_path.exists(), "a no-op remove must not write the file");

        s.add(account(1, "One")).unwrap();
        s.add(account(2, "Two")).unwrap();

        assert!(!s.remove(999).unwrap());
        assert_eq!(ids(&s), vec![1, 2]);

        assert!(s.remove(1).unwrap());
        assert_eq!(ids(&s), vec![2]);

        let reloaded = AccountStore::new(s.file_path.clone());
        reloaded.load().unwrap();
        assert_eq!(ids(&reloaded), vec![2]);

        assert!(!s.remove(1).unwrap(), "removing twice is false the second time");
    }

    // ---- update ---------------------------------------------------------------

    #[test]
    fn update_replaces_the_whole_account_and_reports_a_miss_without_saving() {
        let s = store("update");
        assert!(
            !s.update(account(1, "Ghost")).unwrap(),
            "updating an unknown id must be false"
        );
        assert!(!s.file_path.exists(), "a missed update must not write the file");

        s.add(account(1, "One")).unwrap();
        s.add(account(2, "Two")).unwrap();

        let mut edited = account(1, "Renamed");
        edited.alias = "alias".to_string();
        edited.group = "Group".to_string();
        assert!(s.update(edited).unwrap());

        let stored = s.get_all().unwrap();
        assert_eq!(ids(&s), vec![1, 2], "update must keep the position");
        assert_eq!(stored[0].username, "Renamed");
        assert_eq!(stored[0].alias, "alias");
        assert_eq!(stored[0].group, "Group");

        let reloaded = AccountStore::new(s.file_path.clone());
        reloaded.load().unwrap();
        assert_eq!(reloaded.get_all().unwrap()[0].group, "Group");
    }

    #[test]
    fn group_round_trips_through_the_file_including_the_implicit_default() {
        let s = store("groups");
        s.add(account(1, "Default")).unwrap();
        let mut grouped = account(2, "Grouped");
        grouped.group = "Bots".to_string();
        s.add(grouped).unwrap();

        // The default group is omitted from the JSON but restored on read.
        let raw = fs::read_to_string(&s.file_path).unwrap();
        assert_eq!(raw.matches("\"Group\"").count(), 1, "{raw}");
        assert!(raw.contains("\"Bots\""), "{raw}");

        let reloaded = AccountStore::new(s.file_path.clone());
        reloaded.load().unwrap();
        let loaded = reloaded.get_all().unwrap();
        assert_eq!(loaded[0].group, "Default");
        assert_eq!(loaded[1].group, "Bots");
    }

    // ---- reorder --------------------------------------------------------------

    #[test]
    fn reorder_is_a_no_op_for_an_empty_store_or_an_empty_id_list() {
        let s = store("reorder-noop");
        s.reorder(&[1, 2, 3]).expect("empty store");
        assert!(!s.file_path.exists(), "a no-op reorder must not write the file");

        s.add(account(1, "One")).unwrap();
        s.add(account(2, "Two")).unwrap();
        s.reorder(&[]).expect("empty id list");
        assert_eq!(ids(&s), vec![1, 2]);
    }

    #[test]
    fn reorder_moves_the_listed_ids_to_the_front_and_keeps_the_rest_in_order() {
        let s = store("reorder-partial");
        for id in 1..=5 {
            s.add(account(id, &format!("User{id}"))).unwrap();
        }

        // Only a subset is listed: the rest keeps its relative order behind it.
        s.reorder(&[4, 2]).unwrap();
        assert_eq!(ids(&s), vec![4, 2, 1, 3, 5]);

        // A full list is an exact permutation.
        s.reorder(&[5, 4, 3, 2, 1]).unwrap();
        assert_eq!(ids(&s), vec![5, 4, 3, 2, 1]);

        let reloaded = AccountStore::new(s.file_path.clone());
        reloaded.load().unwrap();
        assert_eq!(ids(&reloaded), vec![5, 4, 3, 2, 1]);
    }

    #[test]
    fn reorder_ignores_unknown_ids_and_repeated_ids_without_losing_accounts() {
        let s = store("reorder-weird");
        for id in 1..=3 {
            s.add(account(id, &format!("User{id}"))).unwrap();
        }

        // Unknown ids are skipped, a repeated id only moves once.
        s.reorder(&[999, 3, 3, -1, 0, 1]).unwrap();
        assert_eq!(ids(&s), vec![3, 1, 2]);

        // Nothing known at all: the list is untouched.
        s.reorder(&[777, 888]).unwrap();
        assert_eq!(ids(&s), vec![3, 1, 2]);
    }

    // ---- import ---------------------------------------------------------------

    #[test]
    fn import_of_empty_data_is_an_empty_summary_and_writes_nothing() {
        let s = store("import-empty");
        let summary = s.import_old_account_data(b"", None).unwrap();
        assert_eq!(
            (summary.total, summary.added, summary.replaced, summary.skipped),
            (0, 0, 0, 0)
        );
        assert!(!s.file_path.exists(), "an empty import must not write the file");

        // An empty JSON array behaves the same way.
        let summary = s.import_old_account_data(b"[]", None).unwrap();
        assert_eq!(summary.total, 0);
        assert!(!s.file_path.exists());
    }

    #[test]
    fn import_counts_added_replaced_and_skipped_entries() {
        let s = store("import-counts");
        s.add(account(1, "Existing")).unwrap();

        let mut duplicate_a = account(2, "DupFirst");
        duplicate_a.alias = "first".to_string();
        let mut duplicate_b = account(2, "DupLast");
        duplicate_b.alias = "last".to_string();

        let payload = serde_json::to_vec(&vec![
            account(1, "Replaced"),
            account(3, "Added"),
            duplicate_a,
            duplicate_b,
            account(0, "InvalidZero"),
            account(-5, "InvalidNegative"),
        ])
        .unwrap();

        let summary = s.import_old_account_data(&payload, None).unwrap();

        assert_eq!(summary.total, 6, "total counts every decoded entry");
        assert_eq!(summary.added, 2, "user 3 and user 2");
        assert_eq!(summary.replaced, 1, "user 1");
        assert_eq!(
            summary.skipped, 3,
            "two non-positive ids plus the duplicated user 2"
        );

        assert_eq!(ids(&s), vec![1, 3, 2], "replacements keep their slot");
        let stored = s.get_all().unwrap();
        assert_eq!(stored[0].username, "Replaced");
        assert_eq!(
            stored[2].alias, "last",
            "the last duplicate wins, the earlier one is the skipped copy"
        );
    }

    #[test]
    fn import_drops_pre_existing_duplicate_user_ids_keeping_the_first() {
        let s = store("import-dedup");
        {
            let mut accounts = s.accounts.lock().unwrap();
            let mut first = account(8, "First");
            first.alias = "first".to_string();
            let mut second = account(8, "Second");
            second.alias = "second".to_string();
            accounts.push(first);
            accounts.push(second);
        }

        let payload = serde_json::to_vec(&vec![account(9, "New")]).unwrap();
        let summary = s.import_old_account_data(&payload, None).unwrap();

        assert_eq!(summary.added, 1);
        assert_eq!(ids(&s), vec![8, 9]);
        assert_eq!(
            s.get_all().unwrap()[0].alias,
            "first",
            "retain() keeps the first occurrence"
        );
    }

    #[test]
    fn import_rejects_data_that_is_neither_json_nor_a_legacy_blob() {
        let s = store("import-garbage");
        let err = s
            .import_old_account_data(b"\x01\x02\x03 not json", None)
            .unwrap_err();
        assert!(
            err.contains("failed plaintext and legacy DPAPI decode"),
            "{err}"
        );
        assert!(!s.file_path.exists());

        // Valid JSON of the wrong shape reports a parse error instead.
        let err = s
            .import_old_account_data(br#"{"accounts": []}"#, None)
            .unwrap_err();
        assert!(err.contains("failed plaintext and legacy DPAPI decode"), "{err}");
    }

    #[test]
    fn import_of_an_encrypted_export_requires_and_validates_the_password() {
        let s = store("import-encrypted");

        let err = s
            .import_old_account_data(encrypted_sample(), None)
            .unwrap_err();
        assert_eq!(err, "IMPORT_PASSWORD_REQUIRED");
        assert!(!s.file_path.exists());

        let err = s
            .import_old_account_data(encrypted_sample(), Some("wrong-password"))
            .unwrap_err();
        assert_eq!(err, "Import password is incorrect");
        assert!(!s.file_path.exists());

        let summary = s
            .import_old_account_data(encrypted_sample(), Some(SAMPLE_PASSWORD))
            .unwrap();
        assert_eq!(summary.added, 1);
        assert_eq!(ids(&s), vec![111]);
        // The import password is not adopted as the store password.
        assert!(!s.is_encrypted().unwrap(), "the store stays plain");
    }

    #[test]
    fn import_trims_the_import_password_just_like_unlocking() {
        // Este teste fixava o comportamento antigo: `load_with_password` fazia
        // trim e `decode_accounts_for_import` não, então a mesma senha colada
        // com espaço no fim desbloqueava o app mas era recusada no import.
        // Agora as duas normalizam igual.
        let s = store("import-trim");
        let summary = s
            .import_old_account_data(encrypted_sample(), Some(&format!("  {SAMPLE_PASSWORD}\t\n")))
            .expect("a senha com espaços em volta deve ser aceita");
        assert_eq!(summary.added, 1);
        assert_eq!(ids(&s), vec![111]);

        // Uma senha realmente diferente continua sendo recusada.
        let other = store("import-trim-wrong");
        assert_eq!(
            other
                .import_old_account_data(encrypted_sample(), Some(" not-the-password "))
                .unwrap_err(),
            "Import password is incorrect"
        );
    }

    #[test]
    fn import_refuses_to_run_after_a_failed_load() {
        let s = store("import-latched");
        fs::write(&s.file_path, b"corrupt").unwrap();
        assert!(s.load().is_err());

        let payload = serde_json::to_vec(&vec![account(1, "New")]).unwrap();
        let err = s.import_old_account_data(&payload, None).unwrap_err();
        assert!(err.contains("refusing to overwrite"), "{err}");
        assert_eq!(fs::read(&s.file_path).unwrap(), b"corrupt");
    }

    #[test]
    fn import_summary_serializes_in_camel_case_for_the_frontend() {
        let summary = OldAccountImportSummary {
            total: 4,
            added: 2,
            replaced: 1,
            skipped: 1,
        };
        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["total"], 4);
        assert_eq!(json["added"], 2);
        assert_eq!(json["replaced"], 1);
        assert_eq!(json["skipped"], 1);
    }

    // ---- decoding helpers -----------------------------------------------------

    #[test]
    fn parse_accounts_json_reports_the_parse_error() {
        let err = AccountStore::parse_accounts_json(b"[{").unwrap_err();
        assert!(err.starts_with("Failed to parse account JSON:"), "{err}");
        assert!(AccountStore::parse_accounts_json(b"[]").unwrap().is_empty());
    }

    /// Substitui o antigo teste de `decode_accounts_for_load`: a decodificação
    /// virou parte de `load_encrypted`, que também decide se o caso é "precisa de
    /// senha" ou "a chave deste aparelho se perdeu".
    #[test]
    fn load_encrypted_asks_for_the_password_without_a_key_file_and_reports_a_bad_secret() {
        let s = store("decode-load");

        // Sem `.key` e sem sessão: é vault de senha.
        let err = s.load_encrypted(encrypted_sample()).unwrap_err();
        assert!(err.contains("Password required"), "{err}");
        assert!(!err.contains(".json.bak"), "não é caso de chave perdida: {err}");

        // Com um segredo em memória que não é o do arquivo, o arquivo em disco não
        // foi escrito por esta sessão (restauração de backup, troca por fora).
        // Além de falhar, isso **tranca** a gravação: sem o latch, o save seguinte
        // cifraria o arquivo novo com o segredo velho e o próximo boot não abriria.
        *s.session.lock().unwrap() = Some(SessionKey::derive("nope-not-it").unwrap());
        let err = s.load_encrypted(encrypted_sample()).unwrap_err();
        assert!(err.to_lowercase().contains("restart"), "{err}");
        assert!(
            s.load_failed.load(std::sync::atomic::Ordering::SeqCst),
            "o latch não foi ligado: uma gravação depois disso perderia o arquivo"
        );
        assert!(s.save().is_err());

        // Texto puro continua sendo lido pelo caminho de sempre.
        let plain = serde_json::to_vec(&vec![account(1, "A")]).unwrap();
        assert_eq!(
            AccountStore::decode_plain_or_legacy_accounts(&plain)
                .unwrap()
                .len(),
            1
        );
    }

    // ---- concurrency ----------------------------------------------------------

    #[test]
    fn concurrent_adds_all_land_in_memory_and_leave_a_readable_file() {
        let s = store("concurrent");
        let store_ref = &s.store;

        std::thread::scope(|scope| {
            for thread in 0..4i64 {
                scope.spawn(move || {
                    for i in 0..5i64 {
                        let id = thread * 100 + i + 1;
                        store_ref.add(account(id, &format!("U{id}"))).unwrap();
                    }
                });
            }
        });

        let mut got = ids(&s);
        got.sort();
        assert_eq!(got.len(), 20, "every add must survive: {got:?}");

        // Este teste aceitava que o arquivo ficasse atrás da memória ("only
        // check it is a subset") porque `save()` reobtinha o lock depois da
        // mutação. Agora a serialização acontece sob o mesmo guard, então o
        // arquivo tem exatamente o que está em memória.
        let mut on_disk: Vec<i64> = serde_json::from_slice::<Vec<Account>>(
            &fs::read(&s.file_path).unwrap(),
        )
        .expect("file stays valid")
        .iter()
        .map(|a| a.user_id)
        .collect();
        on_disk.sort();
        assert_eq!(on_disk, got, "o arquivo não pode ficar atrás da memória");
        assert!(!s.file_path.with_extension("json.tmp").exists());
    }

    #[test]
    fn every_mutation_leaves_the_file_equal_to_the_in_memory_list() {
        // Invariante nova: quando `add`/`update`/`remove`/`reorder` retornam,
        // o arquivo já contém exatamente o snapshot que a mutação produziu —
        // não existe mais janela entre soltar o lock e gravar.
        let s = store("snapshot-invariant");

        let on_disk = |s: &AccountStore| -> Vec<i64> {
            serde_json::from_slice::<Vec<Account>>(&fs::read(&s.file_path).unwrap())
                .expect("arquivo válido")
                .iter()
                .map(|a| a.user_id)
                .collect()
        };

        for id in 1..=4 {
            s.add(account(id, &format!("U{id}"))).unwrap();
            assert_eq!(on_disk(&s), ids(&s));
        }

        assert!(s.update(account(2, "Renamed")).unwrap());
        assert_eq!(on_disk(&s), ids(&s));

        s.reorder(&[4, 1]).unwrap();
        assert_eq!(on_disk(&s), ids(&s));

        assert!(s.remove(1).unwrap());
        assert_eq!(on_disk(&s), ids(&s));
    }

    // ---- derivação de chave ---------------------------------------------------

    #[test]
    fn unlocking_derives_the_key_once_and_every_save_reuses_it() {
        // `save()` chamava `crypto::encrypt`, que sorteia salt e roda argon2i
        // MODERATE (256 MiB) a cada gravação, segurando o lock — mover N contas
        // de grupo congelava a UI N vezes. Agora o argon2 roda só no unlock.
        //
        // O observável direto disso é o salt no arquivo: se a chave fosse
        // re-derivada, cada gravação traria um salt novo.
        let salt_of = |bytes: &[u8]| bytes[crypto::RAM_HEADER.len()..crypto::RAM_HEADER.len() + 16].to_vec();

        let s = store("key-reuse");
        s.load_with_password(SAMPLE_PASSWORD).unwrap();

        s.add(account(1, "One")).unwrap();
        let first = fs::read(&s.file_path).unwrap();
        s.add(account(2, "Two")).unwrap();
        let second = fs::read(&s.file_path).unwrap();
        s.update(account(2, "Two Renamed")).unwrap();
        let third = fs::read(&s.file_path).unwrap();

        assert!(crypto::is_encrypted(&first));
        assert_eq!(salt_of(&first), salt_of(&second), "o salt da sessão é estável");
        assert_eq!(salt_of(&second), salt_of(&third));
        // O nonce, esse sim, continua sorteado a cada gravação.
        let nonce_of = |b: &[u8]| b[crypto::RAM_HEADER.len() + 16..crypto::RAM_HEADER.len() + 40].to_vec();
        assert_ne!(nonce_of(&second), nonce_of(&third), "nonce nunca se repete");

        // O formato do arquivo não mudou: `crypto::decrypt` com o hash da senha
        // continua abrindo, e um store novo relê tudo.
        let decrypted =
            crypto::decrypt(&third, &crypto::hash_password(SAMPLE_PASSWORD)).unwrap();
        let parsed: Vec<Account> = serde_json::from_slice(&decrypted).unwrap();
        assert_eq!(parsed.len(), 2);

        let reloaded = AccountStore::new(s.file_path.clone());
        reloaded.load_with_password(SAMPLE_PASSWORD).unwrap();
        assert_eq!(ids(&reloaded), vec![1, 2]);
        assert_eq!(reloaded.get_all().unwrap()[1].username, "Two Renamed");

        // Cada unlock sorteia um salt novo, então ele não vira uma constante.
        reloaded.add(account(3, "Three")).unwrap();
        assert_ne!(
            salt_of(&fs::read(&s.file_path).unwrap()),
            salt_of(&third),
            "um unlock novo rotaciona o salt"
        );
    }

    #[test]
    fn saving_many_times_never_derives_the_key_again() {
        // Guarda de regressão para a UI travada: a invariante é que **uma
        // gravação não paga argon2** — a chave derivada no unlock é reaproveitada.
        //
        // Até 30/09/2026 isto era medido em tempo ("10 gravações custam menos que
        // 3 derivações") e piscava na CI: cada gravação faz `sync_all()`, e o
        // disco lento do runner contra um argon2 otimizado (dependências em
        // opt-level 3 no perfil de teste) estourava o teto sem regressão nenhuma.
        // Contar derivações prova a invariante sem depender de disco nem de CPU.
        let s = store("save-cost");
        s.load_with_password(SAMPLE_PASSWORD).unwrap();
        s.add(account(1, "One")).unwrap();

        let before = crate::data::crypto::DERIVE_KEY_CALLS.with(|calls| calls.get());
        for id in 2..=11 {
            s.add(account(id, &format!("U{id}"))).unwrap();
        }
        let derivations = crate::data::crypto::DERIVE_KEY_CALLS.with(|calls| calls.get()) - before;

        assert_eq!(ids(&s).len(), 11);
        assert_eq!(
            derivations, 0,
            "10 gravações fizeram {derivations} derivações argon2: cada gravação voltou a pagar o unlock"
        );
    }
}

#[cfg(test)]
mod vault_migration_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Store num arquivo temporário, com limpeza de **todos** os arquivos que o
    /// vault pode deixar: o `.key`, o `.json.bak` e os `.tmp`.
    struct TempVault {
        store: AccountStore,
    }

    impl Drop for TempVault {
        fn drop(&mut self) {
            let path = self.store.file_path.clone();
            for extra in [
                "json.tmp",
                "json.bak",
                "json.rekey.bak",
                "key",
                "key.tmp",
                "key.probe",
            ] {
                let _ = fs::remove_file(path.with_extension(extra));
            }
            let _ = fs::remove_file(&path);
        }
    }

    impl std::ops::Deref for TempVault {
        type Target = AccountStore;
        fn deref(&self) -> &AccountStore {
            &self.store
        }
    }

    fn vault(name: &str) -> TempVault {
        crypto::init();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        TempVault {
            store: AccountStore::new(
                std::env::temp_dir().join(format!("ram-vault-{name}-{nanos}.json")),
            ),
        }
    }

    fn sample_accounts() -> Vec<Account> {
        vec![
            Account::new(
                "_|WARNING:-DO-NOT-SHARE-COOKIE-1".to_string(),
                "Main".to_string(),
                111,
            ),
            Account::new(
                "_|WARNING:-DO-NOT-SHARE-COOKIE-2".to_string(),
                "Alt".to_string(),
                222,
            ),
        ]
    }

    fn bak_path(store: &AccountStore) -> PathBuf {
        store.file_path.with_extension("json.bak")
    }

    fn key_path(store: &AccountStore) -> PathBuf {
        store.file_path.with_extension("key")
    }

    /// Hex só para montar arquivos `.key` no teste.
    fn hex_for_test(bytes: &[u8]) -> String {
        use std::fmt::Write;
        let mut out = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            let _ = write!(out, "{:02x}", byte);
        }
        out
    }

    /// A chave mestra que o `.key` entrega **sem** contar com o DPAPI.
    ///
    /// É assim que se pergunta "o embrulho do aparelho ainda está vivo?": copia o
    /// arquivo sem o campo `dpapi` e tenta abrir a cópia. Não mexe no original.
    fn master_key_without_dpapi_blob(key_path: &std::path::Path) -> Option<Vec<u8>> {
        let raw = fs::read(key_path).ok()?;
        let mut file: serde_json::Value = serde_json::from_slice(&raw).ok()?;
        file.as_object_mut()?.remove("dpapi");
        let probe = key_path.with_extension("key.probe");
        fs::write(&probe, serde_json::to_vec(&file).ok()?).ok()?;
        let recovered = crate::data::vault_key::load_master_key(&probe).map(|r| r.master);
        let _ = fs::remove_file(&probe);
        recovered
    }

    /// Um `AccountData.json` em JSON puro (o formato que deixava o cookie de
    /// todas as contas legível para qualquer programa) tem que virar vault
    /// cifrado na primeira abertura, sem perder uma conta.
    #[test]
    fn a_plain_json_vault_is_migrated_to_encrypted_and_keeps_every_account() {
        let store = vault("migrate-plain");
        let plain = serde_json::to_vec(&sample_accounts()).unwrap();
        fs::write(&store.file_path, &plain).unwrap();

        store.load().expect("abrir um vault em texto puro");

        // 1. O arquivo no disco não é mais texto puro.
        let on_disk = fs::read(&store.file_path).unwrap();
        assert!(
            crypto::is_encrypted(&on_disk),
            "o vault continuou em texto puro"
        );
        assert!(
            !on_disk
                .windows(30)
                .any(|w| w == b"_|WARNING:-DO-NOT-SHARE-COOKIE"),
            "o cookie vazou em texto no vault cifrado"
        );

        // 2. Nenhuma conta se perdeu.
        let loaded = store.get_all().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].user_id, 111);
        assert_eq!(loaded[1].user_id, 222);
        assert_eq!(loaded[0].security_token, "_|WARNING:-DO-NOT-SHARE-COOKIE-1");

        // 3. Um store novo abre o arquivo migrado **sem senha**.
        let reopened = AccountStore::new(store.file_path.clone());
        reopened.load().expect("reabrir o vault migrado");
        assert!(!reopened.needs_password().unwrap());
        assert_eq!(reopened.get_all().unwrap()[1].username, "Alt");
    }

    /// A migração troca o formato do arquivo: é o único momento em que o usuário
    /// pode perder as contas. O `.json.bak` tem que existir **antes** disso.
    #[test]
    fn the_migration_leaves_a_json_bak_copy_of_the_plain_vault() {
        let store = vault("migrate-backup");
        let plain = serde_json::to_vec(&sample_accounts()).unwrap();
        fs::write(&store.file_path, &plain).unwrap();

        store.load().expect("abrir");

        let backup = bak_path(&store);
        assert!(backup.exists(), "a migração não deixou .json.bak");
        assert_eq!(
            fs::read(&backup).unwrap(),
            plain,
            "o .json.bak tem que ser o arquivo de antes, byte a byte"
        );
    }

    /// Quando a chave do aparelho **não** pode ser recuperada, o vault fica como
    /// está e o app reporta onde está o backup. Apagar ou regravar aqui é perder
    /// as contas do usuário — pior que ficar sem criptografia.
    #[test]
    fn an_unrecoverable_device_key_never_touches_the_vault_and_reports_the_backup() {
        let store = vault("unrecoverable");
        let foreign = crypto::device_hash_for_identifier("aparelho-que-nao-existe-mais");
        let json = serde_json::to_string(&sample_accounts()).unwrap();
        let encrypted = crypto::encrypt(&json, &foreign).unwrap();
        fs::write(&store.file_path, &encrypted).unwrap();
        // Um `.key` que só abre com o hash daquele outro aparelho: é o caso
        // "reinstalei o Windows" / "o antivírus trocou o arquivo".
        let device_blob = crypto::encrypt(&"00".repeat(32), &foreign).unwrap();
        fs::write(
            key_path(&store),
            serde_json::json!({ "v": 1, "device": hex_for_test(&device_blob) }).to_string(),
        )
        .unwrap();

        let err = store
            .load()
            .expect_err("não pode abrir com chave irrecuperável");

        // 1. A mensagem diz o que aconteceu e onde procurar a cópia. Qual cópia
        //    ela cita depende de o `.json.bak` existir — isso é o M5, coberto por
        //    `the_locked_vault_message_only_points_at_a_backup_that_exists`.
        assert!(
            err.contains("backup"),
            "a mensagem não diz onde está o backup: {err}"
        );
        assert!(
            err.contains("Nothing was deleted or overwritten"),
            "a mensagem não deixa claro que o arquivo está intacto: {err}"
        );
        // 2. Nada de segredo na mensagem de erro.
        assert!(
            !err.contains("_|WARNING"),
            "cookie na mensagem de erro: {err}"
        );
        // 3. O arquivo está exatamente como estava.
        assert_eq!(fs::read(&store.file_path).unwrap(), encrypted);
        // 4. E nenhuma gravação passa por cima dele.
        assert!(store
            .add(Account::new("c".to_string(), "Nova".to_string(), 999))
            .is_err());
        assert_eq!(fs::read(&store.file_path).unwrap(), encrypted);
    }

    /// Quem já tem senha continua como está: a senha manda, e o arquivo de chave
    /// do aparelho sai de cena (senão o vault abriria sem a senha).
    #[test]
    fn setting_a_password_removes_the_key_file_and_makes_the_vault_need_it() {
        let store = vault("set-password");
        fs::write(
            &store.file_path,
            serde_json::to_vec(&sample_accounts()).unwrap(),
        )
        .unwrap();
        store.load().expect("migrar");
        assert!(
            key_path(&store).exists(),
            "a migração tinha que criar o .key"
        );

        store
            .set_password(Some("senha-bem-comprida"))
            .expect("definir senha");

        assert!(!key_path(&store).exists(), "o .key sobrou depois da senha");
        assert!(crypto::is_encrypted(&fs::read(&store.file_path).unwrap()));

        let reopened = AccountStore::new(store.file_path.clone());
        assert!(
            reopened.load().is_err(),
            "abriu um vault de senha sem a senha"
        );
        assert!(reopened.needs_password().unwrap());
        reopened
            .load_with_password("senha-bem-comprida")
            .expect("destrancar");
        assert_eq!(reopened.get_all().unwrap().len(), 2);
    }

    /// Tirar a senha volta para a chave do aparelho — **não** para texto puro.
    #[test]
    fn clearing_the_password_goes_back_to_the_device_key_not_to_plain_text() {
        let store = vault("clear-password");
        let json = serde_json::to_string(&sample_accounts()).unwrap();
        let encrypted =
            crypto::encrypt(&json, &crypto::hash_password("senha-bem-comprida")).unwrap();
        fs::write(&store.file_path, &encrypted).unwrap();
        store
            .load_with_password("senha-bem-comprida")
            .expect("destrancar");

        store.set_password(None).expect("tirar a senha");

        let on_disk = fs::read(&store.file_path).unwrap();
        assert!(
            crypto::is_encrypted(&on_disk),
            "tirar a senha gravou o vault em texto puro"
        );
        assert!(key_path(&store).exists(), "não criou o .key");

        let reopened = AccountStore::new(store.file_path.clone());
        reopened.load().expect("abrir com a chave do aparelho");
        assert!(!reopened.needs_password().unwrap());
        assert_eq!(reopened.get_all().unwrap().len(), 2);
    }

    /// Instalação nova: a primeira conta já entra num vault cifrado. Sem isto, o
    /// arquivo nasceria em texto puro e a migração nunca aconteceria.
    #[test]
    fn a_brand_new_vault_is_encrypted_from_the_first_account() {
        let store = vault("brand-new");
        store.load().expect("abrir um vault que não existe");

        store
            .add(Account::new(
                "_|WARNING:-DO-NOT-SHARE-NEW".to_string(),
                "Nova".to_string(),
                7,
            ))
            .expect("adicionar");

        let on_disk = fs::read(&store.file_path).unwrap();
        assert!(
            crypto::is_encrypted(&on_disk),
            "vault novo nasceu em texto puro"
        );
        assert!(key_path(&store).exists());
    }

    /// Vault cifrado **sem** `.key` é vault de senha: a ausência do arquivo é o
    /// sinal que evita gastar um argon2 por candidato de aparelho a cada boot.
    #[test]
    fn an_encrypted_vault_without_a_key_file_asks_for_the_password() {
        let store = vault("password-only");
        let json = serde_json::to_string(&sample_accounts()).unwrap();
        let encrypted =
            crypto::encrypt(&json, &crypto::hash_password("senha-bem-comprida")).unwrap();
        fs::write(&store.file_path, &encrypted).unwrap();

        assert!(store.load().is_err());
        assert!(store.needs_password().unwrap());
        assert!(
            !bak_path(&store).exists(),
            "não migrou nada, não podia ter backup"
        );
        assert_eq!(fs::read(&store.file_path).unwrap(), encrypted);
    }

    /// **Critical 1.** O segundo embrulho não pode morrer em silêncio.
    ///
    /// Se o nome do PC muda, o blob `device` fica preso ao nome antigo, mas o
    /// DPAPI continua abrindo — e ninguém percebe. Meses depois o perfil é
    /// recriado (**o caso exato para o qual os dois embrulhos existem**) e não
    /// sobra nenhum caminho. Todo open bem-sucedido tem que deixar os **dois**
    /// embrulhos válidos para os identificadores de agora.
    #[cfg(target_os = "windows")]
    #[test]
    fn opening_the_vault_rewraps_a_device_blob_left_behind_by_an_identifier_change() {
        let store = vault("stale-device-wrapper");
        store.load().expect("abrir vault novo");
        store
            .add(Account::new("cookie".to_string(), "Presa".to_string(), 77))
            .expect("adicionar");

        // Estado de "o PC foi renomeado depois de o .key ser criado": o DPAPI
        // abre, o blob do aparelho está preso a um identificador que não existe.
        let master = crate::data::vault_key::load_master_key(&key_path(&store))
            .expect("recuperar a chave")
            .master;
        let foreign = crypto::device_hash_for_identifier("NOME-ANTIGO-DO-PC");
        crate::data::vault_key::store_master_key(&key_path(&store), &master, &foreign)
            .expect("gravar .key com identificador velho");

        // Pré-condição: sem o DPAPI, esse .key já não abre.
        assert!(
            master_key_without_dpapi_blob(&key_path(&store)).is_none(),
            "o teste não montou o estado que queria"
        );

        // Um boot normal: o DPAPI abre e nada parece errado.
        let reopened = AccountStore::new(store.file_path.clone());
        reopened.load().expect("abrir pelo DPAPI");
        assert_eq!(reopened.get_all().unwrap()[0].user_id, 77);

        // O que este teste protege: o blob do aparelho tem que ter sido
        // regravado com o identificador de agora.
        assert_eq!(
            master_key_without_dpapi_blob(&key_path(&store)).as_deref(),
            Some(master.as_slice()),
            "o embrulho do aparelho continuou preso ao identificador velho"
        );
    }

    /// **Quebra 1, a filha da correção do Critical 1.** O `.key` que **existe mas
    /// não abre** é pior que o que falta: só `!exists` era conferido, então um
    /// `.key` vazio/truncado (queda de energia, `ENOSPC`, antivírus prendendo
    /// handle entre o write e o rename) passava o dia inteiro sem ninguém notar —
    /// o master está na memória — e só aparecia no boot seguinte, como lockout.
    ///
    /// E a janela para isso ficou **por boot** em vez de uma vez na vida do
    /// arquivo, justamente porque a correção do Critical 1 regrava sempre. Esta é
    /// a rede que faltava embaixo dela.
    #[test]
    fn a_save_repairs_a_key_file_that_exists_but_no_longer_opens() {
        let store = vault("key-corrupt");
        store.load().expect("abrir vault novo");
        store
            .add(Account::new("cookie".to_string(), "Antes".to_string(), 1))
            .expect("primeira conta");

        // Queda de energia no meio da regravação: o arquivo ficou, o conteúdo não.
        fs::write(key_path(&store), b"").unwrap();
        assert!(key_path(&store).exists(), "o arquivo continua existindo");
        assert!(
            crate::data::vault_key::load_master_key(&key_path(&store)).is_none(),
            "o teste não montou um .key ilegível"
        );

        store
            .add(Account::new("cookie".to_string(), "Depois".to_string(), 2))
            .expect("gravar com o .key ilegível");

        assert!(
            crate::data::vault_key::load_master_key(&key_path(&store)).is_some(),
            "a gravação não reparou o .key ilegível"
        );
        let reopened = AccountStore::new(store.file_path.clone());
        reopened.load().expect("reabrir depois do reparo");
        assert_eq!(reopened.get_all().unwrap().len(), 2);
    }

    /// **Quebra 1, segunda metade.** `eprintln!` numa build GUI não vai a lugar
    /// nenhum. Quando o reparo do `.key` não é possível, o usuário tem que ficar
    /// sabendo **no dia em que o arquivo fica ruim**, não no boot seguinte.
    #[test]
    fn a_key_file_that_cannot_be_repaired_raises_a_warning_for_the_ui() {
        let store = vault("key-warning");
        store.load().expect("abrir vault novo");
        assert!(store.vault_key_warning().is_none(), "nasceu avisando");

        // Um diretório no lugar do `.key`: ilegível **e** impossível de regravar.
        fs::remove_file(key_path(&store)).unwrap();
        fs::create_dir(key_path(&store)).unwrap();

        // A gravação continua funcionando — travá-la deixaria o usuário sem poder
        // cadastrar conta, e isso não devolve a chave.
        store
            .add(Account::new("cookie".to_string(), "Mesmo assim".to_string(), 9))
            .expect("a gravação não pode parar por causa do .key");

        let warning = store
            .vault_key_warning()
            .expect("a falha não chegou a lugar que a UI possa ler");
        // Estruturado, não frase pronta: é o que permite a UI traduzir.
        // Qual das duas variantes de "writeFailed" depende de como o SO reporta
        // um diretório no lugar do arquivo (no Windows vem como acesso negado, que
        // e indistinguivel de antivirus segurando o handle). O que este teste
        // garante e que a falha **chega** a UI.
        assert!(
            warning.code.starts_with("writeFailed"),
            "codigo inesperado: {}", warning.code
        );
        assert_eq!(warning.path, key_path(&store).display().to_string());
        let dump = format!("{:?}", warning);
        assert!(!dump.contains("cookie"), "segredo no aviso: {dump}");

        let _ = fs::remove_dir(key_path(&store));
    }

    /// **Quebra 2 da 3a rodada.** O reparo perguntava "abre?", não "abre para a
    /// chave **desta sessão**?". Pasta de dados em OneDrive/Dropbox (ou portátil
    /// sincronizado): o `.key` volta a uma versão anterior e o vault fica novo →
    /// `load_master_key` devolve `Some(master antigo)`, nenhum reparo acontece, o
    /// vault segue sendo gravado com o master da sessão, e o boot seguinte pede
    /// uma senha que nunca existiu.
    #[test]
    fn a_save_repairs_a_key_file_that_holds_a_different_master_key() {
        let store = vault("key-wrong-master");
        store.load().expect("abrir vault novo");
        store
            .add(Account::new("cookie".to_string(), "Antes".to_string(), 1))
            .expect("primeira conta");

        // A sincronização de nuvem trouxe de volta um `.key` mais antigo: ele
        // **abre**, mas entrega outra chave mestra.
        let stale_master = crate::data::vault_key::generate_master_key();
        crate::data::vault_key::store_master_key(
            &key_path(&store),
            &stale_master,
            &crypto::primary_device_hash(),
        )
        .expect("plantar .key de outra chave");
        assert!(
            crate::data::vault_key::load_master_key(&key_path(&store)).is_some(),
            "o teste precisa de um .key que ABRE, só com a chave errada"
        );

        store
            .add(Account::new("cookie".to_string(), "Depois".to_string(), 2))
            .expect("gravar");

        // O `.key` tem que ter voltado a ser o desta sessão.
        let now = crate::data::vault_key::load_master_key(&key_path(&store))
            .expect("o .key tem que abrir")
            .master;
        assert_ne!(
            now, stale_master,
            "o .key continuou com a chave errada: o próximo boot pediria uma senha inexistente"
        );

        let reopened = AccountStore::new(store.file_path.clone());
        reopened.load().expect("reabrir");
        assert_eq!(reopened.get_all().unwrap().len(), 2);
    }

    /// **Quebra 4 da 3a rodada.** "Ilegível agora" não é "corrompido". Antivírus
    /// segurando o handle por um instante não pode disparar o aviso mais
    /// assustador que existe sobre um arquivo perfeito — alarme falso treina o
    /// dono a ignorar alarme, e aí a rede da Quebra 1 não vale nada.
    /// **N1: a rede não pode se desligar sozinha.** O braço "o `.key` está
    /// saudável" limpava o slot **inteiro**, inclusive avisos que falam do
    /// `AccountData.json` — e limpava **antes** de a gravação acontecer.
    ///
    /// Sequência que apagava a única rede: a migração falha → faixa vermelha
    /// "continua em texto puro" → qualquer gravação seguinte entra em
    /// `save_locked`, o `.key` está perfeito → aviso apagado → a gravação falha de
    /// novo. E essa "gravação seguinte" pode ser um ciclo de Auto Rejoin, sem o
    /// dono clicar em nada.
    #[test]
    fn a_healthy_key_file_does_not_erase_a_warning_about_the_account_file() {
        let store = vault("scoped-clear");
        store.load().expect("abrir vault novo");
        store
            .add(Account::new("cookie".to_string(), "Antes".to_string(), 1))
            .unwrap();

        // Aviso sobre o **vault**, não sobre a chave.
        store.set_key_warning(VaultKeyWarning::migration_failed(
            &store.file_path,
            "tmp bloqueado",
        ));

        // O `.key` está perfeito, então o braço saudável roda — e não pode levar
        // embora um aviso que não é dele.
        store.clear_key_warning_for(&key_path(&store));

        let warning = store
            .vault_key_warning()
            .expect("o aviso do vault foi apagado pelo braço do .key");
        assert_eq!(warning.code, "migrationFailed");

        // Só o escopo do próprio arquivo limpa.
        store.clear_key_warning_for(&store.file_path);
        assert!(store.vault_key_warning().is_none());
    }

    /// **O mesmo defeito do N1, num segundo sítio.** `refresh_key_file` também
    /// limpava sem escopo, e também **antes** de a gravação do vault acontecer:
    /// com `migrationFailed` no slot, bastava um save cujo `.key` não guardasse o
    /// master atual para a faixa vermelha sumir — e se a gravação falhasse em
    /// seguida, o dono ficava com o arquivo legível e **sem faixa**.
    ///
    /// A invariante é: **nenhum caminho limpa aviso de escopo alheio.**
    #[test]
    fn repairing_the_key_file_does_not_erase_a_warning_about_the_account_file() {
        let store = vault("refresh-scoped-clear");
        store.load().expect("abrir vault novo");

        store.set_key_warning(VaultKeyWarning::migration_failed(
            &store.file_path,
            "a migração falhou antes",
        ));

        // O reparo do `.key` dá certo — e não pode levar junto o aviso do vault.
        let master = crate::data::vault_key::load_master_key(&key_path(&store))
            .expect("recuperar a chave")
            .master;
        assert!(store.refresh_key_file(&master), "o reparo tinha que dar certo");

        let warning = store
            .vault_key_warning()
            .expect("o reparo do .key apagou o aviso do AccountData.json");
        assert_eq!(warning.code, "migrationFailed");
    }

    /// **N2: nunca rebaixar gravidade.** O slot é único e last-write-wins, e os
    /// dois avisos co-ocorrem no mesmo tipo de volume (share de rede, FS de
    /// nuvem): `writeFailed` (vermelho, "faça backup agora") virava
    /// `syncUnconfirmed` (âmbar) 26 linhas depois, no mesmo save.
    #[test]
    fn a_mild_warning_never_replaces_a_severe_one() {
        let store = vault("severity");
        let key = key_path(&store);

        store.set_key_warning(VaultKeyWarning::write_failed(&key, "disco cheio", false));
        store.set_key_warning(VaultKeyWarning::sync_unconfirmed(&store.file_path));
        assert_eq!(
            store.vault_key_warning().unwrap().code,
            "writeFailed",
            "aviso grave foi rebaixado para âmbar"
        );

        // `migrationFailed` também é grave, e um âmbar não o encobre.
        store.clear_key_warning();
        store.set_key_warning(VaultKeyWarning::migration_failed(&store.file_path, "x"));
        store.set_key_warning(VaultKeyWarning::weak_wrapper(&key));
        assert_eq!(store.vault_key_warning().unwrap().code, "migrationFailed");

        // Grave **sobre** grave troca (a informação mais recente vale).
        store.set_key_warning(VaultKeyWarning::write_failed(&key, "outro", false));
        assert_eq!(store.vault_key_warning().unwrap().code, "writeFailed");

        // E âmbar sobre âmbar também troca: nada fica preso.
        store.clear_key_warning();
        store.set_key_warning(VaultKeyWarning::weak_wrapper(&key));
        store.set_key_warning(VaultKeyWarning::sync_unconfirmed(&store.file_path));
        assert_eq!(store.vault_key_warning().unwrap().code, "syncUnconfirmed");
    }

    /// Uma gravação bem-sucedida e cifrada resolve o `migrationFailed`: o arquivo
    /// deixou de estar em texto puro. Mas só **depois** de dar certo.
    #[test]
    fn a_successful_encrypted_save_resolves_the_plain_text_warning() {
        let store = vault("resolve-migration");
        store.load().expect("abrir vault novo");
        store.set_key_warning(VaultKeyWarning::migration_failed(
            &store.file_path,
            "falhou antes",
        ));

        store
            .add(Account::new("cookie".to_string(), "Depois".to_string(), 2))
            .expect("gravar");

        assert!(
            crypto::is_encrypted(&fs::read(&store.file_path).unwrap()),
            "o teste precisa de uma gravação cifrada de verdade"
        );
        assert!(
            store.vault_key_warning().is_none(),
            "o arquivo está cifrado agora; o aviso de texto puro tinha que sair"
        );
    }

    #[test]
    fn a_healthy_save_clears_a_warning_that_was_left_over() {
        let store = vault("key-transient");
        store.load().expect("abrir vault novo");

        store.set_key_warning(VaultKeyWarning::weak_wrapper(&key_path(&store)));
        assert!(store.vault_key_warning().is_some());

        store
            .add(Account::new("cookie".to_string(), "Limpa".to_string(), 8))
            .expect("gravar");

        assert!(
            store.vault_key_warning().is_none(),
            "gravação saudável não limpou o aviso antigo"
        );
    }

    /// **A2 do checkup.** A faixa só era lida no boot, em `loadAccounts` e no erro
    /// de trocar a criptografia. Gravador de fundo — Auto Rejoin a cada ciclo,
    /// Watcher, servidor HTTP — não dispara nenhuma dessas leituras: com o Auto
    /// Rejoin rodando a noite inteira, o `.key` que ficava ruim às 3h gerava o
    /// aviso só no backend, o dono fechava o app de manhã sem ver nada, e o boot
    /// seguinte caía em lockout. Toda mudança do aviso tem que sair do store na
    /// hora, pelo canal que a janela escuta.
    #[test]
    fn a_warning_raised_by_a_background_write_reaches_the_ui_right_away() {
        let store = vault("warning-reaches-ui");
        store.load().expect("abrir vault novo");
        store
            .add(Account::new("cookie".to_string(), "Main".to_string(), 1))
            .expect("adicionar");
        let changes = store.watch_key_warning();

        // Às 3h o `.key` vira algo que não abre nem aceita ser regravado.
        fs::remove_file(key_path(&store)).unwrap();
        fs::create_dir(key_path(&store)).unwrap();

        // Um ciclo do Auto Rejoin grava, sem ninguém na frente da tela.
        store
            .mark_used(1)
            .expect("a gravação não para por causa do .key");

        let raised = changes
            .try_recv()
            .expect("o aviso ficou só no backend: a faixa nunca apareceria")
            .expect("chegou 'sem aviso' no lugar do aviso");
        assert!(raised.code.starts_with("writeFailed"), "{}", raised.code);
        assert_eq!(raised.path, key_path(&store).display().to_string());

        // O ciclo seguinte bate no mesmo problema: a mesma faixa não se repete.
        store.mark_used(1).expect("gravar de novo");
        assert!(
            changes.try_recv().is_err(),
            "o mesmo aviso foi publicado de novo"
        );

        // Resolvido por outra gravação de fundo: a faixa tem que sair sozinha.
        fs::remove_dir(key_path(&store)).unwrap();
        store.mark_used(1).expect("gravar com o .key de volta");
        assert_eq!(
            changes.try_recv().expect("a resolução não chegou à UI"),
            None,
            "a faixa ficaria na tela depois de o problema sumir"
        );
    }

    /// **O fail-open mais grave que sobrou.** Quando a migração falha fora do
    /// `.key` — a cópia de segurança ou a regravação —, o app subia normal, a tela
    /// dizia "Device Key", e o `AccountData.json` continuava em **texto puro** com
    /// o cookie de todas as contas, sem um sinal em lugar nenhum: o `Err` do
    /// `load()` morria num `eprintln!`, o `load_failed` já tinha sido limpo e
    /// `needs_password()` era `false`.
    #[test]
    fn a_failed_migration_warns_that_the_account_file_is_still_plain_text() {
        let store = vault("migration-fails");
        let plain = serde_json::to_vec(&sample_accounts()).unwrap();
        fs::write(&store.file_path, &plain).unwrap();
        // Diretório no lugar do `.json.bak`: a cópia de segurança vai falhar, e sem
        // cópia a migração **aborta** (regra da tarefa).
        fs::create_dir(bak_path(&store)).unwrap();

        let err = store.load().expect_err("a migração tem que falhar");

        // 1. O arquivo ficou **inteiro**, em texto puro.
        assert_eq!(fs::read(&store.file_path).unwrap(), plain);
        // 2. E as contas foram carregadas (o usuário continua usando o app).
        assert_eq!(store.get_all().unwrap().len(), 2);
        // 3. O que este teste protege: existe aviso para a UI mostrar.
        let warning = store
            .vault_key_warning()
            .expect("a migração falhou calada: o arquivo ficou em texto puro sem sinal nenhum");
        assert_eq!(warning.code, "migrationFailed");
        assert_eq!(warning.path, store.file_path.display().to_string());
        assert!(!err.is_empty());
        assert!(
            !format!("{:?}", warning).contains("_|WARNING"),
            "segredo no aviso"
        );

        let _ = fs::remove_dir(bak_path(&store));
    }

    /// **A6 do checkup.** A migração que não consegue criar o `.key` deixava na
    /// faixa o aviso **do `.key`**: `migrationFailed` só entrava com o slot vazio,
    /// e `refresh_key_file` já tinha gravado `writeFailed` (ou o transitório). A
    /// faixa dizia "tento de novo na próxima alteração" — falso: sem sessão, o
    /// `save_locked` nem toca no `.key` — ou "pode não abrir depois de fechar" —
    /// falso: o arquivo está em texto puro e abre. O que importa é o cookie de
    /// todas as contas legível, que é o `migrationFailed`. Ele tem que vencer.
    #[test]
    fn a_migration_that_cannot_create_the_key_file_says_the_file_is_still_plain_text() {
        let store = vault("migration-no-key");
        let plain = serde_json::to_vec(&sample_accounts()).unwrap();
        fs::write(&store.file_path, &plain).unwrap();
        // Diretório no lugar do `.key`: a chave não pode ser criada.
        fs::create_dir(key_path(&store)).unwrap();

        store.load().expect_err("a migração tem que falhar");

        assert_eq!(
            fs::read(&store.file_path).unwrap(),
            plain,
            "o arquivo tinha que ficar inteiro, em texto puro"
        );
        let warning = store
            .vault_key_warning()
            .expect("a migração falhou calada");
        assert_eq!(
            warning.code, "migrationFailed",
            "a faixa fala do .key quando o que importa é o arquivo em texto puro"
        );
        assert_eq!(warning.path, store.file_path.display().to_string());

        let _ = fs::remove_dir(key_path(&store));
    }

    /// **A6, pelo caminho que a própria faixa manda seguir.** O `migrationFailed`
    /// diz "abra Change Encryption Method para tentar de novo". Se a nova
    /// tentativa (`set_password(None)`) bate no mesmo `.key` impossível, o
    /// `writeFailed` dela trocava a faixa (mesma gravidade, o mais novo vence) e
    /// voltava a falar do `.key` — com o arquivo ainda em texto puro.
    #[test]
    fn retrying_the_device_key_while_still_plain_text_keeps_saying_plain_text() {
        let store = vault("retry-still-plain");
        let plain = serde_json::to_vec(&sample_accounts()).unwrap();
        fs::write(&store.file_path, &plain).unwrap();
        fs::create_dir(key_path(&store)).unwrap();
        store.load().expect_err("a migração tem que falhar");

        store
            .set_password(None)
            .expect_err("a chave continua sem poder ser criada");

        assert_eq!(fs::read(&store.file_path).unwrap(), plain);
        assert_eq!(
            store.vault_key_warning().expect("sem aviso").code,
            "migrationFailed",
            "a nova tentativa trocou 'texto puro' por um aviso sobre o .key"
        );

        let _ = fs::remove_dir(key_path(&store));
    }

    /// **Fail-open na guarda que protege a única cópia da chave.**
    /// `is_encrypted().unwrap_or(false)` fazia "não consegui saber" virar "não
    /// existe vault", e é essa guarda que decide se uma chave mestra nova pode ser
    /// sorteada por cima da antiga.
    #[test]
    fn not_knowing_whether_a_vault_exists_counts_as_it_existing() {
        let store = vault("fail-safe-guard");
        // Um diretório no lugar do vault: `is_encrypted()` devolve `Err`.
        fs::create_dir(&store.file_path).unwrap();

        assert!(
            store.is_encrypted().is_err(),
            "o teste precisa de um vault ilegível"
        );
        assert!(
            store.vault_may_hold_data(),
            "não saber se existe vault cifrado tem que contar como 'existe'"
        );

        let _ = fs::remove_dir(&store.file_path);
        // E o caminho normal continua respondendo a verdade.
        assert!(!store.vault_may_hold_data(), "sem arquivo, não há o que perder");
        store.load().expect("abrir vault novo");
        store
            .add(Account::new("cookie".to_string(), "X".to_string(), 1))
            .unwrap();
        assert!(store.vault_may_hold_data(), "agora há vault cifrado");
    }

    /// **Aviso grudado.** Depois de definir senha o `.key` é apagado, então um
    /// aviso sobre ele é falso alarme — e nenhum caminho de sessão-com-senha
    /// alcançava o `clear_key_warning`, então a faixa vermelha ficava na tela o
    /// resto da sessão apontando para um arquivo que não existe mais.
    #[test]
    fn setting_a_password_clears_a_warning_about_a_key_that_no_longer_matters() {
        let store = vault("warning-sticks");
        store.load().expect("abrir vault novo");
        store
            .add(Account::new("cookie".to_string(), "Antes".to_string(), 1))
            .unwrap();
        store.set_key_warning(VaultKeyWarning::write_failed(
            &key_path(&store),
            "acesso negado",
            false,
        ));
        assert!(store.vault_key_warning().is_some());

        store
            .set_password(Some("senha-bem-comprida"))
            .expect("definir senha");

        assert!(!key_path(&store).exists(), "o .key tinha que ter saído");
        assert!(
            store.vault_key_warning().is_none(),
            "faixa vermelha ficou apontando para um .key que acabou de ser apagado"
        );
    }

    /// O canal que existe para observar falhas não pode falhar sem ser observado:
    /// com o mutex envenenado, `if let Ok(..)` descartava a escrita e
    /// `.lock().ok()` respondia "tudo em ordem".
    #[test]
    fn a_poisoned_warning_mutex_still_reports_the_warning() {
        let store = vault("poisoned-warning");
        let path = key_path(&store);

        let _ = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _guard = store.key_warning.lock().unwrap();
                    panic!("envenena o mutex de propósito");
                })
                .join()
        });
        assert!(
            store.key_warning.lock().is_err(),
            "o teste precisa do mutex envenenado"
        );

        store.set_key_warning(VaultKeyWarning::weak_wrapper(&path));
        let warning = store
            .vault_key_warning()
            .expect("mutex envenenado não pode virar 'tudo em ordem'");
        assert_eq!(warning.code, "weakWrapper");

        store.clear_key_warning();
        assert!(store.vault_key_warning().is_none());
    }

    /// **Quebra 3 da 3a rodada.** A inversão do latch trancou o único caminho que
    /// devia recarregar: um `AccountData.json` em texto puro restaurado de backup
    /// passa por `migrate_plain_vault` → `save_locked`, e o próprio latch recusava.
    /// O dono recebia "não foi possível reler" **depois** de a leitura funcionar.
    ///
    /// A regra: o latch existe para impedir que memória **velha** sobrescreva o
    /// arquivo; depois de um `load()` bem-sucedido a memória **é** o arquivo.
    #[test]
    fn a_locked_store_can_still_reload_and_migrate_a_restored_plain_vault() {
        let store = vault("restore-plain-migrates");
        let plain = serde_json::to_vec(&sample_accounts()).unwrap();
        fs::write(&store.file_path, &plain).unwrap();

        // Foi o que a restauração fez: trancou antes de qualquer releitura.
        store.lock_writes_until_restart("o arquivo de contas foi restaurado");

        store
            .load()
            .expect("reler um vault restaurado em texto puro tem que funcionar");

        // Migrou de verdade, não só leu.
        assert!(
            crypto::is_encrypted(&fs::read(&store.file_path).unwrap()),
            "leu mas não migrou"
        );
        assert_eq!(store.get_all().unwrap().len(), 2);
        // E a gravação está liberada, porque a memória é o arquivo.
        store
            .add(Account::new("cookie".to_string(), "Nova".to_string(), 3))
            .expect("depois de reler, pode gravar");
    }

    /// **Quebra 5 da 3a rodada.** A criação do `.key` no **primeiro boot** (e em
    /// `set_password(None)`) descartava o `StoredKeyHealth` e, quando falhava,
    /// morria num `eprintln!` do `lib.rs` — o padrão que a Quebra 1 mandou matar.
    #[test]
    fn a_first_boot_that_cannot_create_the_key_file_warns_instead_of_going_silent() {
        let store = vault("first-boot-no-key");
        // Diretório no lugar do `.key`: a criação vai falhar.
        fs::create_dir(key_path(&store)).unwrap();

        let err = store.load().expect_err("não deve dizer que deu tudo certo");

        assert!(
            err.to_lowercase().contains("key file"),
            "o erro não explica o que falhou: {err}"
        );
        assert!(
            store.vault_key_warning().is_some(),
            "o primeiro boot falhou calado: nada para a UI mostrar"
        );
        // **A6.** Sem chave, o store segue sem segredo e grava as contas em texto
        // puro — é isso que a faixa tem que dizer, como o próprio erro acima diz
        // ("left unencrypted"). O `writeFailed` que ficava aqui falava de um
        // `.key` do qual nada depende ("pode não abrir depois de fechar"), sobre
        // um arquivo que vai abrir porque está legível.
        let warning = store.vault_key_warning().unwrap();
        assert_eq!(warning.code, "migrationFailed", "{}", warning.code);
        assert_eq!(warning.path, store.file_path.display().to_string());

        let _ = fs::remove_dir(key_path(&store));
    }

    /// **Quebra 5.** `.key` que abre mas não é o deste vault não é só órfão de
    /// `set_password`: quem copia um `AccountData.json` antigo por cima, ou
    /// restaura um vault de um zip com a chave de outro, cai aqui **sem nunca ter
    /// tido senha**. Pedir senha e parar é um beco sem saída.
    #[test]
    fn a_vault_locked_by_another_device_key_says_how_to_get_out() {
        let store = vault("foreign-master");
        // Vault cifrado por **outra** chave mestra, com um `.key` local válido.
        let other_master = crate::data::vault_key::generate_master_key();
        let json = serde_json::to_string(&sample_accounts()).unwrap();
        fs::write(
            &store.file_path,
            crypto::encrypt(
                &json,
                &crate::data::vault_key::master_password_hash(&other_master),
            )
            .unwrap(),
        )
        .unwrap();
        crate::data::vault_key::store_master_key(
            &key_path(&store),
            &crate::data::vault_key::generate_master_key(),
            &crypto::primary_device_hash(),
        )
        .unwrap();

        let err = store.load().expect_err("não abre");

        assert!(err.contains("Password required"), "{err}");
        assert!(
            err.contains(&key_path(&store).display().to_string()),
            "não diz qual arquivo de chave restaurar: {err}"
        );
        assert!(
            err.to_lowercase().contains("never set a password"),
            "não cobre quem nunca definiu senha: {err}"
        );
        assert!(
            err.contains("Nothing was deleted or overwritten"),
            "não tranquiliza sobre o arquivo: {err}"
        );
    }

    /// **A4 do checkup.** `set_password(Some)` trocava a sessão pela da senha
    /// **antes** de gravar. Com a gravação falhando (antivírus segurando o
    /// `AccountData.json` na troca), a UI dizia que não tinha aplicado — o
    /// `EncryptionMethod` não mudava e o `.key` continuava no disco —, mas a
    /// próxima gravação de fundo que desse certo cifrava o vault com a senha. No
    /// boot seguinte, `.key` presente com vault de senha dava "if you never set a
    /// password... restore the matching AccountData.key": o conselho errado para
    /// quem só precisava digitar a senha que a tela disse não ter valido.
    #[test]
    fn a_password_that_could_not_be_saved_does_not_take_effect() {
        let store = vault("password-not-saved");
        store.load().expect("abrir vault novo");
        store
            .add(Account::new("cookie".to_string(), "Main".to_string(), 1))
            .expect("adicionar");
        let on_disk = fs::read(&store.file_path).unwrap();

        // Um diretório no lugar do `.json.tmp` faz a gravação falhar antes da
        // troca atômica — como o antivírus segurando o arquivo.
        let tmp = store.file_path.with_extension("json.tmp");
        fs::create_dir(&tmp).unwrap();
        store
            .set_password(Some("senha-bem-comprida"))
            .expect_err("a gravação tinha que falhar");
        fs::remove_dir(&tmp).unwrap();

        assert_eq!(
            fs::read(&store.file_path).unwrap(),
            on_disk,
            "o vault mudou apesar do erro"
        );
        assert!(key_path(&store).exists(), "a senha não valeu: o .key fica");
        assert!(
            !store.has_user_password().unwrap(),
            "a UI disse que não aplicou, mas a sessão já era a da senha"
        );

        // A próxima gravação de fundo (um ciclo do Auto Rejoin)...
        store.mark_used(1).expect("gravar depois");
        // ...continua com a chave do aparelho, como a UI disse.
        let reopened = AccountStore::new(store.file_path.clone());
        reopened
            .load()
            .expect("o boot seguinte tem que abrir sem senha: a senha não foi aplicada");
        assert_eq!(reopened.get_all().unwrap().len(), 1);
    }

    /// O espelho do teste acima: **tirar** a senha que não foi gravada também
    /// não passa a valer. A sessão já era a da chave do aparelho antes da
    /// gravação; com ela falhando, a UI dizia que a senha ficou, e a próxima
    /// gravação de fundo tirava a senha mesmo assim.
    #[test]
    fn removing_the_password_that_could_not_be_saved_does_not_take_effect() {
        let store = vault("password-removal-not-saved");
        store.load().expect("abrir vault novo");
        store
            .add(Account::new("cookie".to_string(), "Main".to_string(), 1))
            .expect("adicionar");
        store.set_password(Some("senha-bem-comprida")).expect("pôr a senha");

        let tmp = store.file_path.with_extension("json.tmp");
        fs::create_dir(&tmp).unwrap();
        store
            .set_password(None)
            .expect_err("a gravação tinha que falhar");
        fs::remove_dir(&tmp).unwrap();

        assert!(
            store.has_user_password().unwrap(),
            "a UI disse que não aplicou, mas a sessão já era a da chave do aparelho"
        );
        assert!(
            !key_path(&store).exists(),
            "o .key da tentativa que falhou ficou ao lado de um vault de senha"
        );

        // A próxima gravação de fundo (um ciclo do Auto Rejoin)...
        store.mark_used(1).expect("gravar depois");
        // ...continua com a senha, como a UI disse.
        let reopened = AccountStore::new(store.file_path.clone());
        let _ = reopened.load();
        assert!(
            reopened.needs_password().unwrap(),
            "o boot seguinte abriu sem senha: a remoção que falhou passou a valer"
        );

        let _ = fs::remove_file(key_path(&store));
    }

    /// Tirar a senha quando o `.key` não pode ser criado não muda nada: o vault
    /// continua cifrado com a senha, e é ela que abre. Um aviso sobre o `.key`
    /// ("pode não abrir depois de fechar — ponha uma senha") para quem já tem
    /// senha é falso, e o erro não pode dizer que o arquivo ficou sem cifra.
    #[test]
    fn removing_the_password_without_a_key_file_keeps_the_password_and_warns_nothing() {
        let store = vault("password-removal-no-key");
        store.load().expect("abrir vault novo");
        store
            .add(Account::new("cookie".to_string(), "Main".to_string(), 1))
            .expect("adicionar");
        store.set_password(Some("senha-bem-comprida")).expect("pôr a senha");
        let with_password = fs::read(&store.file_path).unwrap();
        // Não há `.key` (a senha o apagou), e um diretório no lugar do
        // `.key.tmp` faz a criação falhar — como disco cheio ou antivírus.
        let key_tmp = key_path(&store).with_extension("key.tmp");
        fs::create_dir(&key_tmp).unwrap();

        let err = store
            .set_password(None)
            .expect_err("sem .key não há chave do aparelho para trocar pela senha");

        assert!(
            store.vault_key_warning().is_none(),
            "aviso sobre o .key para quem continua com senha: {:?}",
            store.vault_key_warning()
        );
        assert!(
            !err.contains("unencrypted"),
            "o erro diz que o arquivo ficou sem cifra: {err}"
        );
        assert_eq!(fs::read(&store.file_path).unwrap(), with_password);
        assert!(store.has_user_password().unwrap());

        let _ = fs::remove_dir(&key_tmp);
    }

    /// **Quebra 4.** O `.json.rekey.bak` era cópia cifrada pela chave do aparelho
    /// com o `.key` apagado em seguida: nunca mais abria, ninguém limpava, nada
    /// avisava — o "arquivo com cara de backup que não abre com nada" que o
    /// próprio M7 usou como argumento. Não se cria mais, e sobra antiga é limpa.
    #[test]
    fn setting_a_password_leaves_no_dead_rekey_backup_behind() {
        let store = vault("no-dead-backup");
        let plain = serde_json::to_vec(&sample_accounts()).unwrap();
        fs::write(&store.file_path, &plain).unwrap();
        store.load().expect("migrar");

        // Sobra de uma versão anterior do app: tem que sair de cena.
        let stale = store.file_path.with_extension("json.rekey.bak");
        fs::write(&stale, b"copia-cifrada-que-nao-abre-mais").unwrap();

        store
            .set_password(Some("senha-bem-comprida"))
            .expect("definir senha");

        assert!(
            !stale.exists(),
            "deixou um backup que não abre com nada: {}",
            stale.display()
        );
        // E a cópia que **abre** (texto puro, da migração) continua lá.
        assert_eq!(fs::read(bak_path(&store)).unwrap(), plain);
    }

    /// **Important 2.** O `.key` apagado com o app rodando (antivírus, limpeza de
    /// disco) não pode virar um dia inteiro de gravações que ninguém mais abre —
    /// contaminando também todo backup automático criado depois.
    #[test]
    fn a_save_puts_back_a_key_file_that_vanished_while_the_app_was_running() {
        let store = vault("key-vanished");
        store.load().expect("abrir vault novo");
        store
            .add(Account::new("cookie".to_string(), "Antes".to_string(), 1))
            .expect("primeira conta");

        // O antivírus passa.
        fs::remove_file(key_path(&store)).unwrap();
        assert!(!key_path(&store).exists());

        store
            .add(Account::new("cookie".to_string(), "Depois".to_string(), 2))
            .expect("gravar depois de perder o .key");

        assert!(
            key_path(&store).exists(),
            "a gravação não recolocou o arquivo de chave"
        );

        // E o que ficou no disco abre de verdade num processo novo.
        let reopened = AccountStore::new(store.file_path.clone());
        reopened.load().expect("reabrir depois do sumiço");
        assert_eq!(reopened.get_all().unwrap().len(), 2);
    }

    /// **Important 1 / M6.** Restaurar backup deixa a memória velha: o segredo da
    /// sessão não é mais o do arquivo em disco. Gravar aí (um launch já chama
    /// `mark_used`) cifra o vault restaurado com o segredo antigo e o próximo
    /// boot não abre. Toda gravação tem que ser recusada até reiniciar.
    #[test]
    fn a_restore_latch_refuses_every_write_until_the_app_restarts() {
        let store = vault("restore-latch");
        store.load().expect("abrir vault novo");
        store
            .add(Account::new("cookie".to_string(), "Original".to_string(), 5))
            .expect("adicionar");
        let on_disk = fs::read(&store.file_path).unwrap();

        store.lock_writes_until_restart("AccountData.json foi restaurado de um backup");

        let err = store
            .add(Account::new("cookie".to_string(), "Nova".to_string(), 6))
            .expect_err("gravação depois de restaurar tem que ser recusada");
        assert!(err.to_lowercase().contains("restart"), "{err}");
        assert_eq!(
            fs::read(&store.file_path).unwrap(),
            on_disk,
            "o arquivo restaurado foi sobrescrito"
        );
        // mark_used é o caminho do launch, e é por ele que o bug apareceria.
        assert!(store.mark_used(5).is_err());
        assert!(store.save().is_err());

        // **Quebra 3.** "Somente leitura" tem que ser literal: o latch é lido na
        // **entrada** de `set_password`, não só lá no fundo do `save_locked`.
        // Antes, ele já mexia em arquivo (cópia, e a regravação do `.key` no caso
        // `None`) antes de descobrir que não podia gravar.
        let key_before = fs::read(key_path(&store)).unwrap();
        assert!(store.set_password(Some("senha-bem-comprida")).is_err());
        assert!(store.set_password(None).is_err());
        assert_eq!(fs::read(&store.file_path).unwrap(), on_disk);
        assert_eq!(
            fs::read(key_path(&store)).unwrap(),
            key_before,
            "set_password regravou o .key mesmo com a gravação trancada"
        );
        assert!(
            !store.file_path.with_extension("json.rekey.bak").exists(),
            "set_password deixou cópia mesmo com a gravação trancada"
        );
    }

    /// **Quebra 2.** Restaurar um backup e **não** trancar a gravação desfaz a
    /// restauração em silêncio: um launch grava a lista de antes com o segredo da
    /// sessão, e o arquivo continua abrindo, então o usuário só descobre pelas
    /// contas velhas. O default passou a ser **trancado**, e só o caminho que
    /// realmente releu o arquivo destranca — assim um ramo novo nasce seguro.
    #[test]
    fn a_restore_that_touches_the_accounts_files_locks_writes_by_default() {
        // A decisão de **quais** arquivos trancam mora em `commands/backups.rs`
        // (`restore_touches_accounts`), com teste lá. Aqui se garante o par
        // lock/unlock do store, que é o que dá dente à decisão.
        let store = vault("restore-default-locked");
        store.load().expect("abrir vault novo");
        store
            .add(Account::new("cookie".to_string(), "Antes".to_string(), 3))
            .expect("adicionar");

        store.lock_writes_until_restart("restaurado de um backup");
        assert!(store.save().is_err(), "o default tem que ser trancado");

        store.allow_writes_after_reload();
        store
            .add(Account::new("cookie".to_string(), "Depois".to_string(), 4))
            .expect("depois de reler de verdade, pode gravar");
        assert_eq!(store.get_all().unwrap().len(), 2);
    }

    /// **A1 do checkup.** A trava da restauração só pegava o mutex `write_block`.
    /// Uma gravação que já tinha passado pela checagem — um ciclo do Auto Rejoin
    /// no meio do fsync de `mark_used` — terminava **depois** de a trava ligar, e
    /// o `MoveFileExW` dela caía por cima do vault que a extração acabara de pôr
    /// no disco: a restauração era desfeita em silêncio, ou o `.key` do zip
    /// ficava com um vault de outro master.
    ///
    /// Toda gravação segura `accounts` da checagem da trava até a troca do arquivo
    /// (é o contrato de `save_locked`). "Esperar quem já está gravando" é esperar
    /// esse mutex.
    #[test]
    fn the_restore_latch_waits_for_a_write_already_in_flight() {
        use std::sync::mpsc;
        use std::time::Duration;

        let store = vault("latch-waits-writer");
        store.load().expect("abrir vault novo");
        store
            .add(Account::new("cookie".to_string(), "Main".to_string(), 1))
            .expect("adicionar");

        std::thread::scope(|scope| {
            // A gravação em andamento: já passou pela trava (aberta) e ainda não
            // publicou o arquivo.
            let in_flight = store.accounts.lock().unwrap();

            let (latched_tx, latched_rx) = mpsc::channel();
            let restoring: &AccountStore = &store;
            scope.spawn(move || {
                restoring.lock_writes_until_restart("a backup is being restored");
                latched_tx.send(()).unwrap();
            });

            assert!(
                latched_rx.recv_timeout(Duration::from_millis(300)).is_err(),
                "a trava voltou com uma gravação em andamento: a extração começaria \
                 e a gravação cairia por cima do vault restaurado"
            );

            // A gravação que já estava passando termina — antes da extração.
            store
                .save_locked(&in_flight)
                .expect("a gravação que já tinha passado pela trava termina");
            drop(in_flight);

            latched_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("a trava tem que ligar assim que a gravação em andamento termina");
        });

        // Daqui em diante nada grava: o que a extração puser no disco fica.
        assert!(store.mark_used(1).is_err());
    }

    /// **M5.** A mensagem de pânico não pode mandar restaurar um arquivo que
    /// nunca existiu: numa instalação que nasceu cifrada não há `.json.bak`.
    #[test]
    fn the_locked_vault_message_only_points_at_a_backup_that_exists() {
        let store = vault("message-no-bak");
        let foreign = crypto::device_hash_for_identifier("aparelho-que-nao-existe-mais");
        let json = serde_json::to_string(&sample_accounts()).unwrap();
        fs::write(
            &store.file_path,
            crypto::encrypt(&json, &foreign).unwrap(),
        )
        .unwrap();
        let device_blob = crypto::encrypt(&"00".repeat(32), &foreign).unwrap();
        fs::write(
            key_path(&store),
            serde_json::json!({ "v": 1, "device": hex_for_test(&device_blob) }).to_string(),
        )
        .unwrap();

        // Sem `.json.bak` no disco, a mensagem não pode citá-lo.
        assert!(!bak_path(&store).exists());
        let err = store.load().expect_err("não abre");
        assert!(
            !err.contains(".json.bak"),
            "mandou restaurar um arquivo que não existe: {err}"
        );
        assert!(err.contains("backup"), "ainda tem que falar de backup: {err}");

        // Com o `.json.bak` presente, o caminho dele é a informação mais útil.
        fs::write(bak_path(&store), b"[]").unwrap();
        let err = store.load().expect_err("não abre");
        assert!(err.contains(".json.bak"), "{err}");
    }

    /// **A5 do checkup.** Todo lockout termina na tela de senha, que só tem senha
    /// e Continue — Settings não abre com as contas trancadas. A mensagem mandava
    /// restaurar "from Settings > Misc > Data": uma tela que não abre. O caminho
    /// real é à mão e tem ordem: fechar o app, tirar os dois arquivos do lugar
    /// (guardando), pôr de volta os do backup, abrir de novo. O Settings só serve
    /// **depois** de os arquivos saírem do lugar, com o app reaberto vazio.
    #[test]
    fn the_lockout_message_gives_a_way_out_that_works_from_the_password_screen() {
        let store = vault("lockout-way-out");
        let foreign = crypto::device_hash_for_identifier("aparelho-que-nao-existe-mais");
        let json = serde_json::to_string(&sample_accounts()).unwrap();
        fs::write(&store.file_path, crypto::encrypt(&json, &foreign).unwrap()).unwrap();
        let device_blob = crypto::encrypt(&"00".repeat(32), &foreign).unwrap();
        fs::write(
            key_path(&store),
            serde_json::json!({ "v": 1, "device": hex_for_test(&device_blob) }).to_string(),
        )
        .unwrap();
        let data_dir = store.file_path.parent().unwrap().to_path_buf();
        let backups_dir = data_dir.join(crate::BACKUPS_DIR_NAME);

        for with_plain_copy in [false, true] {
            if with_plain_copy {
                fs::write(bak_path(&store), b"[]").unwrap();
            }
            let err = store.load().expect_err("não abre");
            let at = |needle: &str| {
                err.find(needle)
                    .unwrap_or_else(|| panic!("a mensagem não diz {needle:?}: {err}"))
            };

            let close = at("Close the app");
            let move_out = at(&format!(
                "Move AccountData.json and AccountData.key out of {}",
                data_dir.display()
            ));
            let from_zip = at(&format!("from a backup zip in {}", backups_dir.display()));
            let reopen = at("Open the app again");
            assert!(
                close < move_out && move_out < from_zip && from_zip < reopen,
                "fora de ordem: {err}"
            );
            assert!(
                at("Settings > ") > move_out,
                "manda usar o Settings antes de tirar os arquivos do lugar: {err}"
            );
            assert!(err.contains("Nothing was deleted or overwritten"), "{err}");
            if with_plain_copy {
                assert!(
                    at(&bak_path(&store).display().to_string()) > move_out,
                    "manda pôr a cópia por cima antes de tirar o vault do lugar: {err}"
                );
            }
        }
    }

    /// **M7.** O `.json.bak` da migração é a única cópia realmente recuperável
    /// (texto puro). Definir senha não pode trocá-la por uma cópia cifrada cuja
    /// chave é apagada em seguida.
    #[test]
    fn setting_a_password_keeps_the_plain_text_migration_backup_intact() {
        let store = vault("bak-not-clobbered");
        let plain = serde_json::to_vec(&sample_accounts()).unwrap();
        fs::write(&store.file_path, &plain).unwrap();
        store.load().expect("migrar");
        assert_eq!(fs::read(bak_path(&store)).unwrap(), plain);

        store
            .set_password(Some("senha-bem-comprida"))
            .expect("definir senha");

        assert_eq!(
            fs::read(bak_path(&store)).unwrap(),
            plain,
            "o .json.bak em texto puro foi trocado por uma cópia cifrada sem chave"
        );
    }

    /// **M9.** Morrer entre gravar o vault com a senha e apagar o `.key` deixa um
    /// `.key` órfão. Quem só precisa digitar a senha não pode ver "a chave deste
    /// aparelho não pôde ser recuperada" — e o órfão tem que sair de cena quando
    /// a senha provar que o vault é de senha.
    #[test]
    fn an_orphan_key_file_does_not_disguise_a_password_vault() {
        let store = vault("orphan-key");
        let json = serde_json::to_string(&sample_accounts()).unwrap();
        let encrypted =
            crypto::encrypt(&json, &crypto::hash_password("senha-bem-comprida")).unwrap();
        fs::write(&store.file_path, &encrypted).unwrap();
        // `.key` válido para este aparelho, mas que não abre este vault.
        crate::data::vault_key::store_master_key(
            &key_path(&store),
            &crate::data::vault_key::generate_master_key(),
            &crypto::primary_device_hash(),
        )
        .unwrap();

        let err = store.load().expect_err("não abre sem a senha");
        assert!(
            err.contains("Password required"),
            "com um .key órfão a mensagem culpou a chave em vez de pedir a senha: {err}"
        );

        store
            .load_with_password("senha-bem-comprida")
            .expect("a senha tem que abrir");
        assert!(
            !key_path(&store).exists(),
            "o .key órfão continuou em disco depois de a senha provar que é vault de senha"
        );
    }

    /// Senha errada e "`.key` apagado pelo antivírus" são o mesmo estado em
    /// disco. A mensagem do unlock tem que citar o arquivo de chave, senão quem
    /// nunca teve senha fica tentando senhas para sempre.
    #[test]
    fn a_failed_unlock_without_a_key_file_points_at_the_missing_key() {
        let store = vault("missing-key-hint");
        let json = serde_json::to_string(&sample_accounts()).unwrap();
        let encrypted =
            crypto::encrypt(&json, &crypto::hash_password("senha-bem-comprida")).unwrap();
        fs::write(&store.file_path, &encrypted).unwrap();

        let err = store
            .load_with_password("senha-errada-mesmo")
            .expect_err("senha errada tem que falhar");

        assert!(err.contains("Failed to decrypt"), "{err}");
        assert!(
            err.contains(&key_path(&store).display().to_string()),
            "a mensagem não cita o caminho do arquivo de chave: {err}"
        );
        assert!(
            err.contains("device key file is missing"),
            "a mensagem não explica o segundo motivo: {err}"
        );
        assert!(
            !err.contains("_|WARNING") && !err.contains("senha-errada-mesmo"),
            "segredo na mensagem de erro: {err}"
        );
        assert_eq!(fs::read(&store.file_path).unwrap(), encrypted);
    }

    /// Checa que `err` traz os passos à mão **nesta ordem** — a mesma exigência
    /// do A5: as duas mensagens abaixo também terminam na tela de senha, onde o
    /// Settings não abre, e "restaure de um backup" sem dizer como é beco sem
    /// saída.
    fn assert_manual_restore_in_order(err: &str, move_out: &str, backups_dir: &std::path::Path) {
        let at = |needle: &str| {
            err.find(needle)
                .unwrap_or_else(|| panic!("a mensagem não diz {needle:?}: {err}"))
        };
        let close = at("close the app");
        let out = at(move_out);
        let from_zip = at(&format!("from a backup zip in {}", backups_dir.display()));
        let reopen = at("open the app again");
        assert!(
            close < out && out < from_zip && from_zip < reopen,
            "fora de ordem: {err}"
        );
        assert!(!err.contains("Settings"), "manda para uma tela que não abre: {err}");
    }

    #[test]
    fn a_failed_unlock_without_a_key_file_says_how_to_restore_by_hand() {
        let store = vault("missing-key-by-hand");
        let json = serde_json::to_string(&sample_accounts()).unwrap();
        let encrypted =
            crypto::encrypt(&json, &crypto::hash_password("senha-bem-comprida")).unwrap();
        fs::write(&store.file_path, &encrypted).unwrap();
        let data_dir = store.file_path.parent().unwrap().to_path_buf();

        let err = store
            .load_with_password("senha-errada-mesmo")
            .expect_err("senha errada tem que falhar");

        // O caso comum (senha errada) continua sendo a primeira coisa lida.
        assert!(err.starts_with("Failed to decrypt: wrong password."), "{err}");
        assert_manual_restore_in_order(
            &err,
            &format!("move AccountData.json out of {}", data_dir.display()),
            &data_dir.join(crate::BACKUPS_DIR_NAME),
        );
    }

    #[test]
    fn a_vault_of_another_device_key_says_how_to_restore_by_hand() {
        let store = vault("foreign-master-by-hand");
        let other_master = crate::data::vault_key::generate_master_key();
        let json = serde_json::to_string(&sample_accounts()).unwrap();
        fs::write(
            &store.file_path,
            crypto::encrypt(
                &json,
                &crate::data::vault_key::master_password_hash(&other_master),
            )
            .unwrap(),
        )
        .unwrap();
        crate::data::vault_key::store_master_key(
            &key_path(&store),
            &crate::data::vault_key::generate_master_key(),
            &crypto::primary_device_hash(),
        )
        .unwrap();
        let data_dir = store.file_path.parent().unwrap().to_path_buf();

        let err = store.load().expect_err("não abre");

        assert_manual_restore_in_order(
            &err,
            &format!(
                "move AccountData.json and AccountData.key out of {}",
                data_dir.display()
            ),
            &data_dir.join(crate::BACKUPS_DIR_NAME),
        );
        let _ = fs::remove_file(key_path(&store));
    }

    /// "Importar backup encriptado por padrão (sem senha) tem que funcionar."
    ///
    /// O backup do app leva o `AccountData.key` junto com o `AccountData.json`
    /// (`DATA_FILES`), e é justamente por isso que isto funciona: o vault é
    /// cifrado por uma chave mestra aleatória, então sem a chave do backup
    /// nenhuma senha do mundo abre o arquivo. O teste reproduz o par restaurado.
    #[test]
    fn a_default_encrypted_export_imports_back_without_a_password() {
        let source = vault("import-device-encrypted");
        source.load().expect("abrir vault novo");
        source
            .add(Account::new(
                "cookie".to_string(),
                "Exportada".to_string(),
                42,
            ))
            .expect("adicionar");
        let exported = fs::read(&source.file_path).unwrap();
        let exported_key = fs::read(key_path(&source)).unwrap();
        assert!(crypto::is_encrypted(&exported));

        let target = vault("import-device-target");
        // Restaurar é colocar os **dois** arquivos de volta.
        fs::write(key_path(&target), &exported_key).unwrap();
        target.load().expect("abrir vault novo");

        let summary = target
            .import_old_account_data(&exported, None)
            .expect("importar sem senha");

        assert_eq!(summary.added, 1);
        assert_eq!(target.get_all().unwrap()[0].user_id, 42);
    }

    /// E o contrário: um vault cifrado por outra chave mestra **não** abre. A
    /// mensagem é `IMPORT_PASSWORD_REQUIRED` (a UI pede a senha), não um import
    /// silencioso de zero contas.
    #[test]
    fn an_export_from_another_install_is_not_imported_silently() {
        let foreign_master = crate::data::vault_key::generate_master_key();
        let json = serde_json::to_string(&sample_accounts()).unwrap();
        let foreign_vault = crypto::encrypt(
            &json,
            &crate::data::vault_key::master_password_hash(&foreign_master),
        )
        .unwrap();

        let target = vault("import-foreign");
        target.load().expect("abrir vault novo");

        let err = target
            .import_old_account_data(&foreign_vault, None)
            .expect_err("não pode importar vault de outra instalação");
        assert_eq!(err, IMPORT_PASSWORD_REQUIRED);
        assert!(target.get_all().unwrap().is_empty());
    }
}

/// `replace_token_if`: o cookie novo que o Roblox devolve só entra se a conta
/// ainda tiver o cookie que foi enviado (ver `api::cookie_rotation`).
#[cfg(test)]
mod account_token_swap_tests {
    use super::*;

    fn temp_store(tag: &str) -> AccountStore {
        crypto::init();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        AccountStore::new(std::env::temp_dir().join(format!("ram-token-swap-{tag}-{nanos}.json")))
    }

    fn token_of(store: &AccountStore, user_id: i64) -> String {
        store
            .get_all()
            .unwrap()
            .into_iter()
            .find(|a| a.user_id == user_id)
            .unwrap()
            .security_token
    }

    #[test]
    fn the_token_is_replaced_when_the_old_one_matches() {
        let store = temp_store("match");
        let mut account = Account::new("OLD".into(), "one".into(), 1);
        account.valid = false;
        store.add(account).unwrap();

        assert!(store.replace_token_if(1, "OLD", "NEW").unwrap());
        let saved = store.get_all().unwrap().remove(0);
        assert_eq!(saved.security_token, "NEW");
        assert!(saved.valid, "a fresh cookie from Roblox means a live session");
    }

    /// A conta ganhou outro cookie entre o pedido e a resposta (novo login):
    /// o da resposta velha não pode passar por cima.
    #[test]
    fn a_newer_token_is_not_overwritten() {
        let store = temp_store("newer");
        store.add(Account::new("NEWER".into(), "one".into(), 1)).unwrap();

        assert!(!store.replace_token_if(1, "OLD", "ROTATED").unwrap());
        assert_eq!(token_of(&store, 1), "NEWER");
    }

    #[test]
    fn an_unknown_account_is_left_alone() {
        let store = temp_store("unknown");
        store.add(Account::new("OLD".into(), "one".into(), 1)).unwrap();
        assert!(!store.replace_token_if(2, "OLD", "NEW").unwrap());
        assert_eq!(token_of(&store, 1), "OLD");
    }

    /// `set_valid` (ideia 9, "Check accounts"): só muda — e só grava — quando
    /// o valor é outro.
    #[test]
    fn set_valid_only_reports_a_real_change() {
        let store = temp_store("valid");
        store.add(Account::new("TOKEN".into(), "one".into(), 1)).unwrap();

        assert!(store.set_valid(1, false).unwrap());
        assert!(!store.get_all().unwrap()[0].valid);
        assert!(!store.set_valid(1, false).unwrap(), "same value is not a change");
        assert!(store.set_valid(1, true).unwrap());
        assert!(!store.set_valid(99, false).unwrap(), "unknown account");
        assert_eq!(token_of(&store, 1), "TOKEN", "the cookie is never touched");
    }
}

#[cfg(test)]
mod app_lock_verify_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Temp(AccountStore);

    impl Drop for Temp {
        fn drop(&mut self) {
            let path = self.0.file_path.clone();
            let _ = fs::remove_file(&path);
            let _ = fs::remove_file(path.with_extension("key"));
            let _ = fs::remove_file(path.with_extension("json.bak"));
        }
    }

    fn temp(tag: &str) -> Temp {
        crypto::init();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let store = AccountStore::new(std::env::temp_dir().join(format!("ram-lock-{tag}-{nanos}.json")));
        store.load().expect("new vault");
        store
            .add(Account::new("COOKIE".to_string(), "Main".to_string(), 1))
            .expect("add");
        Temp(store)
    }

    #[test]
    fn the_right_password_unlocks_and_a_wrong_one_does_not() {
        let t = temp("verify");
        t.0.set_password(Some("senha-bem-comprida")).expect("set password");
        assert_eq!(t.0.verify_password("senha-bem-comprida"), Ok(true));
        assert_eq!(t.0.verify_password("  senha-bem-comprida  "), Ok(true), "trimmed like the unlock");
        assert_eq!(t.0.verify_password("outra-senha"), Ok(false));
        assert_eq!(t.0.verify_password(""), Ok(false));
    }

    #[test]
    fn checking_never_touches_the_accounts_in_memory_or_on_disk() {
        let t = temp("untouched");
        t.0.set_password(Some("senha-bem-comprida")).expect("set password");
        let before = fs::read(&t.0.file_path).unwrap();
        // Uma mudança que só existe em memória (gravação de fundo em andamento)
        // não pode ser trocada pelo que está no disco.
        t.0.accounts.lock().unwrap()[0].alias = "in memory only".to_string();

        assert_eq!(t.0.verify_password("errada"), Ok(false));
        assert_eq!(t.0.verify_password("senha-bem-comprida"), Ok(true));

        assert_eq!(t.0.get_all().unwrap()[0].alias, "in memory only");
        assert_eq!(fs::read(&t.0.file_path).unwrap(), before, "verifying wrote to the file");
        assert!(t.0.has_user_password().unwrap(), "the session is still the password one");
    }

    #[test]
    fn without_an_app_password_there_is_nothing_to_verify() {
        let t = temp("no-password");
        assert!(t.0.verify_password("anything").is_err());
    }
}