import { useState } from "react";
import { useApp } from "./App";
import { Banner, open, Spinner } from "./components";

function CopyLink({ url }: { url?: string }) {
  const [copied, setCopied] = useState(false);
  if (!url) return null;
  return (
    <div className="copy-link">
      <div className="muted small">브라우저가 열리지 않았다면 이 주소를 복사해 브라우저에 붙여넣으세요. 로그인이 끝나면 앱이 자동으로 연결됩니다.</div>
      <div className="row">
        <input readOnly value={url} onFocus={(e) => e.currentTarget.select()} aria-label="로그인 주소" />
        <button onClick={() => void navigator.clipboard?.writeText(url).then(() => { setCopied(true); setTimeout(() => setCopied(false), 1500); })}>{copied ? "복사됨 ✓" : "주소 복사"}</button>
      </div>
    </div>
  );
}

/** ChatGPT sign-in for the bundled Codex. Used on the chat screen and in Settings. */
export default function CodexLogin({ compact = false }: { compact?: boolean }) {
  const { codex, login, loginError, startLogin, cancelLogin, connectCodex, refreshAccount } = useApp();
  const [copied, setCopied] = useState(false);

  if (codex.state === "starting") return <Spinner label="Codex 준비 중…" />;
  if (codex.state === "error") {
    return (
      <div className="login-box">
        <Banner>{codex.error}</Banner>
        <div className="row"><button className="primary" onClick={() => void connectCodex()}>다시 연결</button></div>
      </div>
    );
  }
  if (codex.account) return null;

  return (
    <div className="login-box">
      {!compact && (
        <p className="muted">
          ChatGPT 계정으로 로그인하면 AI 대화와 브리핑을 쓸 수 있습니다. 로그인은 OpenAI 공식 페이지에서 진행되고,
          로그인 정보는 Windows 자격 증명 관리자에 저장됩니다. 이 앱은 비밀번호를 받지 않습니다.
        </p>
      )}
      {loginError && <Banner>{loginError}</Banner>}

      {login?.mode === "browser" && (
        <div className="device">
          <Spinner label="브라우저에서 로그인을 마치면 자동으로 연결됩니다…" />
          <div className="row wrap">
            <button onClick={() => open(login.authUrl)}>로그인 페이지 다시 열기</button>
            <button className="ghost" onClick={() => void cancelLogin()}>취소</button>
          </div>
          <CopyLink url={login.authUrl} />
        </div>
      )}

      {login?.mode === "device" && (
        <div className="device">
          <div className="muted small">로그인 페이지에서 아래 코드를 입력하세요</div>
          <div className="row">
            <strong className="code">{login.userCode}</strong>
            <button onClick={() => {
              if (login.userCode) void navigator.clipboard?.writeText(login.userCode).then(() => { setCopied(true); setTimeout(() => setCopied(false), 1500); });
            }}>{copied ? "복사됨 ✓" : "복사"}</button>
          </div>
          <div className="row wrap">
            <button className="primary" onClick={() => open(login.verificationUrl)}>로그인 페이지 열기</button>
            <button className="ghost" onClick={() => void cancelLogin()}>취소</button>
          </div>
          {login.openFailed && <CopyLink url={login.verificationUrl} />}
          <Spinner label="코드 입력을 기다리는 중…" />
        </div>
      )}

      {!login && (
        <div className="row wrap">
          <button className="primary" onClick={() => void startLogin("browser")}>ChatGPT로 로그인 (브라우저)</button>
          <button onClick={() => void startLogin("device")}>기기 코드로 로그인</button>
          <button className="ghost" onClick={() => void refreshAccount()}>상태 새로고침</button>
        </div>
      )}
    </div>
  );
}
