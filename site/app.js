// PhitPup website: language switch, release info, the hero demo and the
// "try it" box. The try-it box runs PhitPup's own core (phitpup.wasm) in the
// browser; nothing typed here leaves the page.

"use strict";

const REPO = document.documentElement.dataset.repo;
const RELEASES = `https://github.com/${REPO}/releases`;
const PORTABLE = `${RELEASES}/latest/download/phitpup.exe`;
const SETUP = `${RELEASES}/latest/download/PhitPup-Setup.exe`;

// ---------------------------------------------------------------- language

// Thai is written in the HTML; English lives here.
const EN = {
  "nav.try": "Try it",
  "nav.how": "How it works",
  "nav.install": "Install",
  "nav.faq": "FAQ",
  "hero.eyebrow": "For Windows 10 and 11",
  "hero.title": "Typed in the wrong layout?<br>Fixed in a <em>blink</em>.",
  "hero.lead": "Forgot to switch to Thai and got <code>l;ylfu</code>? PhitPup sees you meant <b>สวัสดี</b> and fixes it on the spot. No deleting, no retyping.",
  "hero.download": "Download free",
  "hero.try": "Try it in the browser",
  "hero.meta": "Easy install, no admin rights needed · about 2.5 MB · open source (MIT)",
  "try.title": "Try it",
  "try.lead": "Type Thai while your keyboard is still on English (or the other way round). It all runs in your browser; nothing is sent anywhere.",
  "try.layout": "Thai layout",
  "layout.ked": "Kedmanee",
  "layout.pat": "Pattachote",
  "try.placeholder": "Try typing l;ylfu 8iy[ or ้ำสสน",
  "try.samples": "Examples:",
  "mode.suggest": "Suggest",
  "mode.auto": "Auto",
  "mode.manual": "Hotkey",
  "hint.suggest": "A bubble shows up as you type. <kbd>Tab</kbd> accepts it, <kbd>Esc</kbd> hides it · <kbd>Shift</kbd>+<kbd>Backspace</kbd> fixes the last word or the selection.",
  "hint.auto": "Press <kbd>Space</kbd> and a word PhitPup is sure about is fixed for you · <kbd>Shift</kbd>+<kbd>Backspace</kbd> right after puts it back.",
  "hint.manual": "Nothing pops up. <kbd>Shift</kbd>+<kbd>Backspace</kbd> fixes the last word; press again to reach further back. Select text first to fix all of it.",
  "modes.title": "Choose how much help you want",
  "modes.suggest.t": "Suggest as you type",
  "modes.suggest.p": "A small bubble appears under the caret when a word looks like the wrong layout. Tab if it is right, otherwise just keep typing. This is the default.",
  "modes.auto.t": "Fix automatically",
  "modes.auto.p": "Press Space and the wrong word is fixed for you, but only when PhitPup is very sure. If it got it wrong, Shift+Backspace right away brings your word back.",
  "modes.manual.t": "On the hotkey",
  "modes.manual.p": "Fixes the last word; press again to go back one word at a time. Select text and press it to fix the whole selection. Works in every mode.",
  "facts.switch": "Switches your keyboard layout too, so you can keep typing",
  "facts.layouts": "Kedmanee and Pattachote Thai layouts",
  "facts.apps": "Works almost everywhere: browsers, LINE, Discord, Word, Notepad",
  "facts.off": "Turns itself off in password fields, password managers, Remote Desktop and full-screen games",
  "privacy.title": "What you type never leaves your computer",
  "privacy.p": "The program that sees your keys has no networking code at all. Only a separate updater goes online, and only when you click Check for updates, to ask for the newest version number and fetch the installer. PhitPup tells wrong-layout words apart with small statistics tables built into the program, remembers only the word you are typing, forgets it as soon as you click or switch windows, and all of its code is open to read.",
  "install.title": "Set up in a minute",
  "install.1.t": "Download the installer",
  "install.1.p": "PhitPup-Setup.exe installs for your account only, with no admin rights. PhitPup then shows up in the Start menu and can be removed from Windows Settings.",
  "install.portable": "Or use the single file, nothing to install (phitpup.exe)",
  "install.1.b": "Download the latest version",
  "install.hash": "SHA-256:",
  "install.2.t": "Run the installer",
  "install.2.p": "If a blue \"Windows protected your PC\" window appears, click <b>More info</b>, then <b>Run anyway</b>. PhitPup does not have a paid code-signing certificate yet. Then click Next until it is done.",
  "install.3.t": "Start typing",
  "install.3.p": "The orange p icon sits next to the clock. Click it to open the settings, pick a mode and make PhitPup start with Windows.",
  "faq.title": "Questions",
  "faq.av.q": "Why does my antivirus warn about it?",
  "faq.av.a": "To know what you typed, PhitPup has to see the keys you press, and to fix a word it has to type for you. Keyloggers use the same Windows features, so some antivirus programs flag anything that does this. All the code is open, and you can check that your file is exactly the one GitHub built by comparing the SHA-256 above.",
  "faq.smart.q": "SmartScreen says the publisher is unknown. Is it safe?",
  "faq.smart.a": "Windows shows this for every program without a paid code-signing certificate. Every release is built by GitHub Actions from the public source code, with a SHA-256 you can check.",
  "faq.wrong.q": "What if it changes a word I meant?",
  "faq.wrong.a": "Press Shift+Backspace right away to get your word back. If a word keeps getting flagged, such as an acronym, press Shift+Esc while the bubble shows it and it will not be suggested again. You can also add words on the Words page.",
  "faq.games.q": "Will it get in the way of games?",
  "faq.games.a": "No. PhitPup turns itself off in full-screen windows, and you can add any program where it should do nothing on the Programs page.",
  "faq.mac.q": "Is there a macOS or phone version?",
  "faq.mac.a": "Windows only for now. The core is already separate from the Windows-specific parts, so macOS is possible later.",
  "faq.remove.q": "How do I remove it?",
  "faq.remove.a": "Open Windows Settings, go to Apps &gt; Installed apps, pick PhitPup and click Uninstall. If you use the single file, turn off Start with Windows, right-click the icon, choose Quit and delete phitpup.exe. Settings live in %APPDATA%\\PhitPup; delete that folder too.",
  "footer.made": "Made by",
  "footer.source": "Source code",
  "status.fixed": "Fixed",
  "status.undone": "Put back",
  "status.nothing": "Nothing to fix",
  "status.loading": "Loading PhitPup…",
  "status.failed": "The try-it box could not load in this browser.",
  "meta.release": (v, size) => `Version ${v} · ${size} · installs without admin rights · open source (MIT)`,
  "meta.portable": (v, size) => `Version ${v} · ${size} · one file, nothing to install · open source (MIT)`,
};

const TH_EXTRA = {
  "hint.auto": "กด <kbd>Space</kbd> แล้วคำที่มั่นใจว่าผิดจะถูกแก้ให้เอง · กด <kbd>Shift</kbd>+<kbd>Backspace</kbd> ทันทีเพื่อคืนคำเดิม",
  "hint.manual": "ไม่มีอะไรเด้งขึ้นมา กด <kbd>Shift</kbd>+<kbd>Backspace</kbd> เพื่อแก้คำล่าสุด กดซ้ำเพื่อย้อนไปทีละคำ หรือเลือกข้อความก่อนเพื่อแก้ทั้งหมด",
  "status.fixed": "แก้แล้ว",
  "status.undone": "คืนคำเดิมแล้ว",
  "status.nothing": "ไม่มีอะไรต้องแก้",
  "status.loading": "กำลังโหลดผิดปุ๊บ…",
  "status.failed": "ช่องทดลองโหลดไม่ได้ในเบราว์เซอร์นี้",
  "meta.release": (v, size) => `รุ่น ${v} · ${size} · ติดตั้งโดยไม่ต้องใช้สิทธิ์ admin · โอเพนซอร์ส MIT`,
  "meta.portable": (v, size) => `รุ่น ${v} · ${size} · ไฟล์เดียวไม่ต้องติดตั้ง · โอเพนซอร์ส MIT`,
};

const TH = { ...TH_EXTRA };
document.querySelectorAll("[data-i18n]").forEach((el) => {
  TH[el.dataset.i18n] ??= el.innerHTML;
});
document.querySelectorAll("[data-i18n-placeholder]").forEach((el) => {
  TH[el.dataset.i18nPlaceholder] ??= el.placeholder;
});

let lang = "th";
const t = (key, ...args) => {
  const v = (lang === "en" ? EN : TH)[key] ?? TH[key] ?? key;
  return typeof v === "function" ? v(...args) : v;
};

function setLang(next) {
  lang = next;
  document.documentElement.lang = next;
  document.querySelectorAll("[data-i18n]").forEach((el) => {
    el.innerHTML = t(el.dataset.i18n);
  });
  document.querySelectorAll("[data-i18n-placeholder]").forEach((el) => {
    el.placeholder = t(el.dataset.i18nPlaceholder);
  });
  document.getElementById("lang").textContent = next === "th" ? "EN" : "ไทย";
  document.title =
    next === "th"
      ? "ผิดปุ๊บ PhitPup: แก้ข้อความที่พิมพ์ผิดภาษา ไทย ⇄ อังกฤษ"
      : "PhitPup: fix text typed in the wrong layout, Thai ⇄ English";
  showRelease();
  updateHint();
  try {
    localStorage.setItem("lang", next);
  } catch {}
}

document.getElementById("lang").addEventListener("click", () => setLang(lang === "th" ? "en" : "th"));

// ---------------------------------------------------------------- release

// { version, sha256, size, setup: { sha256, size } } written by the Pages
// workflow. Releases before 0.3.1 have no installer; then the download
// buttons fetch the single file.
let release = null;

function showRelease() {
  if (!release) return;
  const file = release.setup ?? release;
  const mb = (file.size / 1048576).toFixed(1);
  const meta = release.setup ? "meta.release" : "meta.portable";
  document.getElementById("meta").textContent = t(meta, release.version, `${mb} MB`);
  document.getElementById("sha").textContent = file.sha256;
  for (const id of ["download", "download-2"]) {
    document.getElementById(id).href = release.setup ? SETUP : PORTABLE;
  }
}

for (const id of ["download", "download-2"]) document.getElementById(id).href = PORTABLE;
document.getElementById("portable").href = PORTABLE;
document.getElementById("source").href = `https://github.com/${REPO}`;

// Ask the server every time: a cached copy would show the previous version
// for up to ten minutes after a release.
fetch("release.json", { cache: "no-cache" })
  .then((r) => (r.ok ? r.json() : null))
  .then((r) => {
    release = r;
    showRelease();
  })
  .catch(() => {});

// ---------------------------------------------------------------- hero demo

(function demo() {
  const text = document.getElementById("demo-text");
  const bubble = document.getElementById("demo-bubble");
  const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const typed = "l;ylfu";
  const fixed = "สวัสดี";
  if (reduced) {
    text.textContent = fixed;
    return;
  }
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  (async () => {
    for (;;) {
      text.textContent = "";
      for (const c of typed) {
        text.textContent += c;
        await wait(140 + Math.random() * 90);
      }
      await wait(250);
      bubble.classList.add("show");
      await wait(1300);
      bubble.classList.remove("show");
      text.textContent = fixed;
      await wait(2600);
    }
  })();
})();

// ---------------------------------------------------------------- try it

const input = document.getElementById("input");
const mirror = document.getElementById("mirror");
const bubbleEl = document.getElementById("bubble");
const status = document.getElementById("status");
let mode = "suggest";
let core = null;
let shown = null; // { start, end, fixed, lang }
let dismissed = false;
let undo = null; // { start, before, after } of the last fix, until the next key

function updateHint() {
  const hint = document.getElementById("hint");
  hint.innerHTML = t(`hint.${mode}`);
}

document.querySelectorAll(".seg button").forEach((b) =>
  b.addEventListener("click", () => {
    document.querySelectorAll(".seg button").forEach((x) => x.classList.toggle("on", x === b));
    mode = b.dataset.mode;
    updateHint();
    refresh();
    input.focus();
  }),
);

document.querySelectorAll("[data-sample]").forEach((b) =>
  b.addEventListener("click", () => {
    input.value = b.dataset.sample;
    input.focus();
    input.setSelectionRange(input.value.length, input.value.length);
    undo = null;
    dismissed = false;
    refresh();
  }),
);

document.getElementById("layout").addEventListener("change", refresh);

function say(key) {
  status.textContent = t(key);
  clearTimeout(say.timer);
  say.timer = setTimeout(() => (status.textContent = ""), 1800);
}

async function loadCore() {
  status.textContent = t("status.loading");
  try {
    const bytes = await fetch("phitpup.wasm").then((r) => {
      if (!r.ok) throw new Error(r.status);
      return r.arrayBuffer();
    });
    const { instance } = await WebAssembly.instantiate(bytes, {});
    const x = instance.exports;
    const enc = new TextEncoder();
    const dec = new TextDecoder();
    const call = (fn, text) => {
      const data = enc.encode(text);
      const ptr = x.alloc(data.length);
      new Uint8Array(x.memory.buffer, ptr, data.length).set(data);
      const n = x[fn](Number(document.getElementById("layout").value), ptr, data.length);
      x.dealloc(ptr, data.length);
      if (!n) return null;
      const out = dec.decode(new Uint8Array(x.memory.buffer, x.out_ptr(), n));
      const tab = out.indexOf("\t");
      return { lang: out.slice(0, tab), text: out.slice(tab + 1) };
    };
    core = call;
    status.textContent = "";
  } catch {
    status.textContent = t("status.failed");
  }
}
loadCore();

/** Start of the word that ends at `end` (no whitespace inside). */
function wordStart(text, end) {
  let i = end;
  while (i > 0 && !/\s/.test(text[i - 1])) i--;
  return i;
}

function hideBubble() {
  shown = null;
  bubbleEl.hidden = true;
}

function refresh() {
  if (!core || mode === "manual" || input.selectionStart !== input.selectionEnd) return hideBubble();
  const end = input.selectionStart;
  const text = input.value;
  if (end === 0 || /\s/.test(text[end - 1])) {
    dismissed = false;
    return hideBubble();
  }
  const start = wordStart(text, end);
  const fix = core("suggest", text.slice(start, end));
  if (!fix) {
    dismissed = false;
    return hideBubble();
  }
  if (dismissed) return hideBubble();
  shown = { start, end, fixed: fix.text, lang: fix.lang };
  document.getElementById("bubble-lang").textContent = fix.lang.toUpperCase();
  document.getElementById("bubble-word").textContent = fix.text;
  placeBubble(start);
  bubbleEl.hidden = false;
}

/** Puts the bubble under the start of the word, using a hidden copy of the
 * textarea to find where that character sits. */
function placeBubble(start) {
  mirror.textContent = input.value.slice(0, start);
  const mark = document.createElement("span");
  mark.textContent = "​";
  mirror.appendChild(mark);
  mirror.scrollTop = input.scrollTop;
  const x = mark.offsetLeft;
  const y = mark.offsetTop - input.scrollTop + mark.offsetHeight;
  const max = input.clientWidth - bubbleEl.offsetWidth - 8;
  bubbleEl.style.left = `${Math.max(8, Math.min(x - 10, max))}px`;
  bubbleEl.style.top = `${y + 8}px`;
}

/** Replaces text[start..end] with `insert`, remembering how to undo it. */
function replace(start, end, insert) {
  const before = input.value.slice(start, end);
  input.setRangeText(insert, start, end, "end");
  undo = { start, before, after: insert };
  hideBubble();
  say("status.fixed");
}

input.addEventListener("keydown", (e) => {
  if (!core) return;
  const plain = !e.ctrlKey && !e.altKey && !e.metaKey;

  if (e.key === "Backspace" && e.shiftKey && plain) {
    e.preventDefault();
    const pos = input.selectionStart;
    if (undo && input.value.slice(undo.start, undo.start + undo.after.length) === undo.after) {
      input.setRangeText(undo.before, undo.start, undo.start + undo.after.length, "end");
      undo = null;
      hideBubble();
      dismissed = true;
      return say("status.undone");
    }
    if (input.selectionStart !== input.selectionEnd) {
      const fix = core("selection", input.value.slice(input.selectionStart, input.selectionEnd));
      return fix ? replace(input.selectionStart, input.selectionEnd, fix.text) : say("status.nothing");
    }
    let end = pos;
    while (end > 0 && /\s/.test(input.value[end - 1])) end--;
    const start = wordStart(input.value, end);
    const fix = start < end ? core("manual", input.value.slice(start, end)) : null;
    return fix ? replace(start, end, fix.text) : say("status.nothing");
  }

  if (shown && plain && !e.shiftKey && e.key === "Tab") {
    e.preventDefault();
    return replace(shown.start, shown.end, shown.fixed);
  }
  if (shown && e.key === "Escape") {
    e.preventDefault();
    dismissed = true;
    return hideBubble();
  }

  if (mode === "auto" && e.key === " " && plain) {
    const end = input.selectionStart;
    const start = wordStart(input.value, end);
    if (input.selectionStart === input.selectionEnd && start < end) {
      const fix = core("auto", input.value.slice(start, end));
      if (fix) {
        e.preventDefault();
        replace(start, end, `${fix.text} `);
        undo.before += " "; // the swallowed Space comes back too
        return;
      }
    }
  }

});

// Any edit by the user ends the chance to undo (our own edits fire no event).
input.addEventListener("input", () => {
  undo = null;
  refresh();
});
input.addEventListener("click", () => {
  undo = null;
  refresh();
});
input.addEventListener("keyup", (e) => {
  if (e.key.startsWith("Arrow") || e.key === "Home" || e.key === "End") refresh();
});
input.addEventListener("blur", hideBubble);
input.addEventListener("scroll", () => shown && placeBubble(shown.start));

// ---------------------------------------------------------------- start

let saved = null;
try {
  saved = localStorage.getItem("lang");
} catch {}
setLang(saved ?? (navigator.language.toLowerCase().startsWith("th") ? "th" : "en"));
