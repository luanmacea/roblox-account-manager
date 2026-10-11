import { useEffect, useRef } from "react";
import { idleLongEnough } from "../utils/inactivityLock";

/** O que conta como "a pessoa mexeu na janela do MultiAlt". */
const ACTIVITY_EVENTS = ["pointerdown", "pointermove", "keydown", "wheel", "touchstart"] as const;

/** De quanto em quanto tempo a inatividade é conferida. */
export const INACTIVITY_CHECK_MS = 10_000;

/**
 * Chama `onLock` depois de `minutes` sem interação na janela. Minimizada ou
 * atrás de outra janela, nada chega aqui — conta como inatividade, que é o
 * que se quer. Desligado (`active` falso) ou já trancado, não faz nada.
 */
export function useInactivityLock(active: boolean, minutes: number, locked: boolean, onLock: () => void): void {
  const lastActivity = useRef(Date.now());
  const onLockRef = useRef(onLock);
  onLockRef.current = onLock;

  useEffect(() => {
    if (!active || locked) return;
    lastActivity.current = Date.now();
    const mark = () => {
      lastActivity.current = Date.now();
    };
    for (const name of ACTIVITY_EVENTS) window.addEventListener(name, mark, { passive: true, capture: true });
    const timer = window.setInterval(() => {
      if (idleLongEnough(lastActivity.current, Date.now(), minutes)) onLockRef.current();
    }, INACTIVITY_CHECK_MS);
    return () => {
      for (const name of ACTIVITY_EVENTS) window.removeEventListener(name, mark, { capture: true });
      window.clearInterval(timer);
    };
  }, [active, minutes, locked]);
}
