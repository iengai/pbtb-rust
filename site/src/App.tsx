import { Link, Navigate, Outlet, Route, Routes } from "react-router-dom";
import { useAuth } from "./auth/AuthProvider";
import { Layout } from "./components/Layout";
import { useT } from "./i18n/locale";
import { Account } from "./pages/Account";
import { AddBot } from "./pages/AddBot";
import { BotDetail } from "./pages/BotDetail";
import { Bots } from "./pages/Bots";
import { Callback } from "./pages/Callback";
import { ConfigDetail } from "./pages/ConfigDetail";
import { Configs } from "./pages/Configs";
import { Login } from "./pages/Login";
import { Returns } from "./pages/Returns";

function RequireAuth() {
  const { session } = useAuth();
  return session ? <Outlet /> : <Navigate to="/" replace />;
}

function NotFound() {
  const t = useT();
  return (
    <div className="msg">
      {t.common.notFound} <Link to="/bots">{t.common.backToBots}</Link>
    </div>
  );
}

export function App() {
  return (
    <Routes>
      <Route path="/" element={<Login />} />
      <Route path="/callback" element={<Callback />} />
      <Route element={<Layout />}>
        <Route path="/configs" element={<Configs />} />
        <Route path="/configs/:name" element={<ConfigDetail />} />
        <Route path="/returns" element={<Returns />} />
        <Route element={<RequireAuth />}>
          <Route path="/bots" element={<Bots />} />
          <Route path="/bots/new" element={<AddBot />} />
          <Route path="/bots/:id" element={<BotDetail />} />
          <Route path="/account" element={<Account />} />
        </Route>
        <Route path="*" element={<NotFound />} />
      </Route>
    </Routes>
  );
}
