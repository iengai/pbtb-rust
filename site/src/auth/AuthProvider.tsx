import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import {
  accessToken,
  beginLogin,
  clearSession,
  loadSession,
  type LoginReason,
  type Session,
  takeLoginReason,
} from "./oauth";
import { setAuthHandlers } from "../api/client";

type Auth = {
  session: Session | null;
  reason: LoginReason | null;
  login: () => Promise<void>;
  signOut: () => void;
  // The API refused the token: remember why, and drop it unless the token is
  // fine and only the account is missing.
  refuse: (reason: LoginReason) => void;
  adopt: (s: Session) => void;
  clearReason: () => void;
};

const Ctx = createContext<Auth | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [session, setSession] = useState<Session | null>(() => loadSession());
  const [reason, setReason] = useState<LoginReason | null>(() => takeLoginReason());

  const refuse = useCallback((why: LoginReason) => {
    // A verified token with no account keeps its session: the signup page
    // needs exactly that token to create the account.
    if (why !== "unlinked") {
      clearSession();
      setSession(null);
    }
    setReason(why);
  }, []);
  const signOut = useCallback(() => refuse("signed_out"), [refuse]);
  const adopt = useCallback((s: Session) => {
    setSession(s);
    setReason(null);
  }, []);
  const clearReason = useCallback(() => setReason(null), []);

  // The API client reads the token — renewing it when it has expired — and
  // reports refusals through these rather than through React, so a fetch
  // outside a component (a poll) behaves the same.
  setAuthHandlers({ token: accessToken, refuse });

  const value = useMemo<Auth>(
    () => ({ session, reason, login: beginLogin, signOut, refuse, adopt, clearReason }),
    [session, reason, signOut, refuse, adopt, clearReason],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useAuth(): Auth {
  const v = useContext(Ctx);
  if (!v) throw new Error("useAuth outside AuthProvider");
  return v;
}
