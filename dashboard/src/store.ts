// =============================================
// Store reativa minimalista
// =============================================
//
// Padrao: get / set / subscribe. Toda view se inscreve uma vez no boot
// e re-renderiza quando o estado muda.

type Listener<T> = (state: T) => void;

export interface Store<T extends object> {
  get(): T;
  set(patch: Partial<T>): void;
  subscribe(l: Listener<T>): () => void;
}

export function createStore<T extends object>(initial: T): Store<T> {
  let state = initial;
  const listeners = new Set<Listener<T>>();

  return {
    get: () => state,
    set: (patch) => {
      state = { ...state, ...patch };
      listeners.forEach((l) => l(state));
    },
    subscribe: (l) => {
      listeners.add(l);
      l(state);
      return () => {
        listeners.delete(l);
      };
    },
  };
}

// === LocalStorage helpers (persistir tweaks) ===

const TWEAKS_KEY = 'tcc_tweaks';

interface Tweaks {
  theme: string;
  accent: string;
  density: string;
  scheduleStyle: string;
  estadoOverride: string;
}

export function loadTweaks(): Partial<Tweaks> {
  try {
    const raw = localStorage.getItem(TWEAKS_KEY);
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

export function saveTweaks(tweaks: Tweaks): void {
  try {
    localStorage.setItem(TWEAKS_KEY, JSON.stringify(tweaks));
  } catch {
    /* localStorage cheio ou indisponivel -- ignora silenciosamente */
  }
}
