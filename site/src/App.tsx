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
import { Showcase } from "./pages/Showcase";
import { ShowcaseBot } from "./pages/ShowcaseBot";
import { Signup } from "./pages/Signup";

function RequireAuth() {
  const { session, reason } = useAuth();
  if (!session) return <Navigate to="/" replace />;
  // Signed in, but the API knows no account for this subject: every page
  // behind here would only be refused again.
  if (reason === "unlinked") return <Navigate to="/signup" replace />;
  return <Outlet />;
}

function NotFound() {
  const t = useT();
  const { session } = useAuth();
  return (
    <div className="msg">
      {t.common.notFound}{" "}
      {session ? <Link to="/bots">{t.common.backToBots}</Link> : <Link to="/p">{t.common.backToShowcase}</Link>}
    </div>
  );
}

// The sign-up and callback pages stand outside the header: a signed-in
// subject with no account yet has nowhere else to go.
export function App() {
  return (
    <Routes>
      <Route path="/callback" element={<Callback />} />
      <Route path="/signup" element={<Signup />} />
      <Route element={<Layout />}>
        <Route path="/" element={<Login />} />
        <Route path="/configs" element={<Configs />} />
        <Route path="/configs/:name" element={<ConfigDetail />} />
        <Route path="/p" element={<Showcase />} />
        <Route path="/p/bots/:pid" element={<ShowcaseBot />} />
        <Route element={<RequireAuth />}>
          <Route path="/bots" element={<Bots />} />
          <Route path="/returns" element={<Returns />} />
          <Route path="/bots/new" element={<AddBot />} />
          <Route path="/bots/:id" element={<BotDetail />} />
          <Route path="/account" element={<Account />} />
        </Route>
        <Route path="*" element={<NotFound />} />
      </Route>
    </Routes>
  );
}
