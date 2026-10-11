// Teto de memória (commands/memory_ceiling.rs): o único ponto que pede ao
// Windows para tirar da RAM as páginas que um cliente não está usando agora.
// Elas vão para o arquivo de paginação e voltam quando o cliente precisar —
// nada é fechado nem perdido.
//
// Só existe com a feature `memory-trim` (nas duas edições, via `standard`): é uma API nativa
// nova no binário (`K32EmptyWorkingSet`, do kernel32 — a mesma família do
// `K32GetProcessMemoryInfo` que o Watcher já usa). Sem a feature, a função
// devolve `false` e o teto fica desligado.

/// Pede ao Windows para liberar a memória do processo `pid`. Devolve se o
/// Windows aceitou.
#[cfg(feature = "memory-trim")]
pub fn trim_working_set(pid: u32) -> bool {
    use windows_sys::Win32::System::Threading::{
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA,
    };

    extern "system" {
        fn K32EmptyWorkingSet(process: HANDLE) -> i32;
    }

    if pid == 0 {
        return false;
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA, 0, pid);
        if handle.is_null() {
            return false;
        }
        let ok = K32EmptyWorkingSet(handle) != 0;
        CloseHandle(handle);
        ok
    }
}

#[cfg(not(feature = "memory-trim"))]
pub fn trim_working_set(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod win_memory_trim_tests {
    use super::*;

    /// PID 0 (o processo ocioso do sistema) nunca é tocado.
    #[test]
    fn pid_zero_is_never_trimmed() {
        assert!(!trim_working_set(0));
    }

    /// A API nativa só entra no binário com a feature, e só neste arquivo.
    #[test]
    fn the_native_call_lives_only_here_and_behind_the_feature() {
        let source = include_str!("memory_trim.rs");
        let code = source.split("#[cfg(test)]").next().unwrap_or(source);
        let at = code.find("K32EmptyWorkingSet(process").expect("the declaration");
        let gate = code[..at].rfind("#[cfg(feature = \"memory-trim\")]").expect("feature gate");
        assert!(code[gate..at].contains("pub fn trim_working_set"));

        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![src];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs")
                    && !path.ends_with("memory_trim.rs")
                {
                    let text = std::fs::read_to_string(&path).unwrap();
                    assert!(
                        !text.contains("EmptyWorkingSet") && !text.contains("SetProcessWorkingSetSize"),
                        "{} calls the trim API outside memory_trim.rs",
                        path.display()
                    );
                }
            }
        }
    }
}
