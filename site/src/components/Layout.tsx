import { NavLink, Outlet } from "react-router-dom";
import { useAuth } from "../auth/AuthProvider";
import { LangSwitch, useT } from "../i18n/locale";

export function Layout() {
  const { session } = useAuth();
  const t = useT();
  const who = session?.claims.email ?? session?.claims.sub ?? null;
  return (
    <>
      <header className="nav">
        <NavLink to="/bots" className="nav-brand">
          <div className="logo" />
          <div className="name">PBTB Console</div>
        </NavLink>
        <nav className="nav-links">
          <NavLink to="/bots" className={({ isActive }) => (isActive ? "on" : "")}>
            {t.common.nav.bots}
          </NavLink>
          <NavLink to="/configs" className={({ isActive }) => (isActive ? "on" : "")}>
            {t.common.nav.configs}
          </NavLink>
          <NavLink to="/account" className={({ isActive }) => (isActive ? "on" : "")}>
            {t.common.nav.account}
          </NavLink>
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
