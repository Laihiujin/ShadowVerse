import "./styles.css";
import App from "./App.svelte";

function escapeHtml(input: string) {
  return input
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

function renderStartupError(raw: unknown) {
  const target = document.getElementById("app");
  if (!target) return;
  const message =
    raw instanceof Error
      ? `${raw.name}: ${raw.message}\n${raw.stack || ""}`
      : String(raw);
  target.innerHTML = `
    <div style="padding:16px;font-family:ui-monospace,SFMono-Regular,Menlo,monospace;background:#111;color:#f8f8f2;min-height:100vh;box-sizing:border-box;">
      <h2 style="margin:0 0 12px 0;font-size:18px;">Frontend Startup Error</h2>
      <pre style="white-space:pre-wrap;word-break:break-word;line-height:1.45;">${escapeHtml(message)}</pre>
    </div>
  `;
}

window.addEventListener("error", (event) => {
  renderStartupError(event.error || event.message || "Unknown runtime error");
});

window.addEventListener("unhandledrejection", (event) => {
  renderStartupError(event.reason || "Unhandled promise rejection");
});

const target = document.getElementById("app");
if (!target) {
  throw new Error("Cannot find #app mount element");
}

const app = new App({ target });

export default app;
