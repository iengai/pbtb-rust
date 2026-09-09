import { type FormEvent, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api, ApiError } from "../api/client";
import { useAction } from "../api/hooks";
import { Key } from "../components/icons";
import { Crumbs, ErrorBanner } from "../components/ui";
import { useT } from "../i18n/locale";

// The three-step add flow: name, key, secret. A 409 for an existing name is
// shown as an overwrite confirmation and retried with `overwrite: true`.
export function AddBot() {
  const t = useT();
  const navigate = useNavigate();
  const [step, setStep] = useState(0);
  const [name, setName] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [secret, setSecret] = useState("");
  const [conflict, setConflict] = useState<string | null>(null);
  const action = useAction();

  const egress = import.meta.env.VITE_EGRESS_IP as string | undefined;
  const steps = [t.bots.add.steps.name, t.bots.add.steps.apiKey, t.bots.add.steps.secret];

  const submit = (overwrite: boolean) =>
    action.run(async () => {
      try {
        const r = await api.addBot({
          name: name.trim(),
          api_key: apiKey.trim(),
          secret_key: secret.trim(),
          ...(overwrite ? { overwrite: true } : {}),
        });
        navigate(`/bots/${encodeURIComponent(r.bot.bot_id)}`, { replace: true });
      } catch (e) {
        if (e instanceof ApiError && e.status === 409 && e.body.status === "already_exists") {
          setConflict(name.trim());
          return;
        }
        throw e;
      }
    });

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (step < 2) {
      setStep(step + 1);
      return;
    }
    void submit(false);
  };

  const canContinue =
    step === 0 ? name.trim().length > 0 : step === 1 ? apiKey.trim().length > 0 : secret.trim().length > 0;

  return (
    <>
      <Crumbs items={[{ to: "/bots", label: t.common.nav.bots }, { label: t.bots.add.title }]} />
      <div style={{ maxWidth: 560 }}>
        <h1 style={{ marginBottom: 4 }}>{t.bots.add.title}</h1>
        <div className="sub" style={{ fontSize: 13.5, marginBottom: 22 }}>
          {t.bots.add.lead}
        </div>
        <div className="stepper">
          {steps.map((s, i) => (
            <span key={s} style={{ display: "contents" }}>
              {i > 0 && <div className="line" />}
              <div className="step">
                <div className={`n${i < step ? " done" : i === step ? " now" : ""}`}>{i + 1}</div>
                <span className={`t${i <= step ? " on" : ""}`}>{s}</span>
              </div>
            </span>
          ))}
        </div>
        <form className="card" style={{ padding: 20 }} onSubmit={onSubmit}>
          <div className="form">
            <div className="field">
              <label htmlFor="bot-name">{t.bots.add.nameLabel}</label>
              {step === 0 ? (
                <input
                  id="bot-name"
                  className="input"
                  value={name}
                  onChange={(e) => {
                    setName(e.target.value);
                    setConflict(null);
                  }}
                  autoFocus
                  autoComplete="off"
                />
              ) : (
                <div className="input ro" style={{ display: "flex", alignItems: "center", color: "var(--text)" }}>
                  {name}
                </div>
              )}
            </div>
            {step >= 1 && (
              <div className="field">
                <label htmlFor="bot-key">{t.bots.add.keyLabel}</label>
                {step === 1 ? (
                  <input
                    id="bot-key"
                    className="input mono"
                    value={apiKey}
                    onChange={(e) => setApiKey(e.target.value)}
                    placeholder={t.bots.add.keyPlaceholder}
                    autoFocus
                    autoComplete="off"
                    spellCheck={false}
                  />
                ) : (
                  <div className="input ro mono" style={{ display: "flex", alignItems: "center" }}>
                    {apiKey.slice(0, 4)}…{apiKey.slice(-4)}
                  </div>
                )}
                <div className="hint" style={{ marginTop: 6 }}>
                  {t.bots.add.keyHint(
                    egress ? <span className="mono">{egress}</span> : t.bots.add.egressFallback,
                  )}
                </div>
              </div>
            )}
            {step >= 2 && (
              <div className="field">
                <label htmlFor="bot-secret">{t.bots.add.secretLabel}</label>
                <input
                  id="bot-secret"
                  className="input mono"
                  type="password"
                  value={secret}
                  onChange={(e) => setSecret(e.target.value)}
                  placeholder={t.bots.add.secretPlaceholder}
                  autoFocus
                  autoComplete="off"
                />
                <div className="hint" style={{ marginTop: 6 }}>
                  {t.bots.add.secretHint}
                </div>
              </div>
            )}
            <div className="form-actions">
              <button
                type="button"
                className="btn ghost"
                onClick={() => (step === 0 ? navigate("/bots") : setStep(step - 1))}
                disabled={action.busy}
              >
                {t.common.back}
              </button>
              {conflict ? (
                <button type="button" className="btn primary" disabled={action.busy} onClick={() => void submit(true)}>
                  {t.bots.add.replaceKey}
                </button>
              ) : (
                <button type="submit" className="btn primary" disabled={!canContinue || action.busy}>
                  {step < 2 ? t.bots.continueLabel : t.bots.add.title}
                </button>
              )}
            </div>
          </div>
        </form>
        {conflict && (
          <div className="banner" style={{ marginTop: 16 }}>
            <div className="ico">
              <Key />
            </div>
            <div className="body">{t.bots.add.conflict(conflict)}</div>
          </div>
        )}
        <div style={{ marginTop: 16 }}>
          <ErrorBanner error={action.error} onDismiss={action.clear} />
        </div>
      </div>
    </>
  );
}
