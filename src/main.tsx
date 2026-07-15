import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

// macOS WKWebView 버그 회피: 화살표 등 기능키를 (특히 길게) 누르면 기능키가 제어문자/PUA 문자
// (□로 보임)로 입력창에 삽입된다. beforeinput 취소는 네이티브 삽입을 못 막으므로, 삽입된 뒤
// 값에서 그런 문자를 제거하고 React 상태에도 반영한다(네이티브 setter + input 재발생).
function stripBadChars(s: string): string {
  let out = "";
  for (const ch of s) {
    const c = ch.codePointAt(0) ?? 0;
    const bad =
      (c < 0x20 && c !== 0x09 && c !== 0x0a && c !== 0x0d) || // 제어문자(탭·개행 제외)
      (c >= 0x7f && c <= 0x9f) || // C1 제어문자
      (c >= 0xe000 && c <= 0xf8ff); // PUA(맥 기능키 표현 U+F700~ 포함)
    if (!bad) out += ch;
  }
  return out;
}
document.addEventListener(
  "input",
  (e) => {
    const el = e.target as HTMLInputElement | HTMLTextAreaElement | null;
    if (!el || (el.tagName !== "INPUT" && el.tagName !== "TEXTAREA")) return;
    const cleaned = stripBadChars(el.value);
    if (cleaned === el.value) return;
    const proto =
      el.tagName === "INPUT" ? HTMLInputElement.prototype : HTMLTextAreaElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
    if (setter) setter.call(el, cleaned);
    else el.value = cleaned;
    // React onChange가 정리된 값을 반영하도록 input 이벤트를 다시 발생(이미 정리돼 재귀 안 함).
    el.dispatchEvent(new Event("input", { bubbles: true }));
  },
  true,
);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
