import { NavLink, Outlet } from "react-router-dom";
import { useAuth } from "../auth/AuthProvider";
import { LangSwitch, useT } from "../i18n/locale";

// The header every page shares. Signed out it offers only what needs no
// account (the catalogue, the showcase, the sign-in) and its brand link lands
// on the showcase; signed in, the account's pages and Bots as home.
export function Layout() {
  const { session } = useAuth();
  const t = useT();
  const who = session?.claims.email ?? session?.claims.sub ?? null;
  const on = ({ isActive }: { isActive: boolean }) => (isActive ? "on" : "");
  return (
    <>
      <header className="nav">
        <NavLink to={session ? "/bots" : "/p"} className="nav-brand">
          <div className="logo" />
          <div className="name">PBTB Console</div>
        </NavLink>
        <nav className="nav-links">
          {session && (
            <NavLink to="/bots" className={on}>
              {t.common.nav.bots}
            </NavLink>
          )}
          <NavLink to="/configs" className={on}>
            {t.common.nav.configs}
          </NavLink>
          {session && (
            <NavLink to="/returns" className={on}>
              {t.common.nav.returns}
            </NavLink>
          )}
          <NavLink to="/p" className={on}>
            {t.common.nav.showcase}
          </NavLink>
          {session && (
            <NavLink to="/account" className={on}>
              {t.common.nav.account}
            </NavLink>
          )}
        </nav>
        <div className="nav-user">
          <LangSwitch />
          <div className="avatar" />
          {who ? (
            <span className="ellipsis" style={{ maxWidth: 260 }}>
              {who}
            </span>
          ) : (
            <NavLink to="/">{t.common.nav.signIn}</NavLink>
          )}
        </div>
      </header>
      <main className="page">
        <Outlet />
      </main>
    </>
  );
}
