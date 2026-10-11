import { useEffect, useRef } from "react";
import { PasswordScreen } from "./PasswordScreen";
import { useTr } from "../../i18n/text";

/**
 * O app trancado por inatividade (ideia 27): a tela de senha **por cima** de
 * tudo, sem desmontar nada por baixo — Scripts, o lote de avatares e os
 * ouvintes de eventos seguem vivos, e o AFK/reconexão (backend) nem ficam
 * sabendo. O resto da janela fica `inert` (em `App.tsx`) e as teclas que não
 * nascem aqui não chegam aos atalhos de lá.
 */
export function LockOverlay() {
  const t = useTr();
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const block = (e: KeyboardEvent) => {
      if (ref.current && e.target instanceof Node && ref.current.contains(e.target)) return;
      e.stopImmediatePropagation();
      e.preventDefault();
    };
    window.addEventListener("keydown", block, true);
    return () => window.removeEventListener("keydown", block, true);
  }, []);

  return (
    <div
      ref={ref}
      role="dialog"
      aria-modal="true"
      aria-label={t("MultiAlt is locked")}
      data-testid="lock-overlay"
      className="theme-app fixed inset-0 z-[1000] overflow-auto"
      // Esc, Ctrl+A e afins digitados no campo de senha não podem chegar aos
      // atalhos da janela (que ouvem no `window`).
      onKeyDown={(e) => e.stopPropagation()}
    >
      <PasswordScreen mode="lock" />
    </div>
  );
}
