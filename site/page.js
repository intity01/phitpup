// The language switch for the plain pages (what's new, privacy). Each page
// carries both languages; data-lang marks which is which.

"use strict";

(function () {
  let lang = "th";
  try {
    lang = localStorage.getItem("lang") || "th";
  } catch {}
  const button = document.getElementById("lang");
  function set(next) {
    lang = next;
    document.documentElement.lang = next;
    button.textContent = next === "th" ? "EN" : "ไทย";
    try {
      localStorage.setItem("lang", next);
    } catch {}
  }
  button.addEventListener("click", () => set(lang === "th" ? "en" : "th"));
  set(lang);
})();
