// Tiny translation layer. Each language is a file in lang/ that fills NBW_LANG[code].
// To add a language: copy lang/en.js to lang/<code>.js, translate the values, add a
// <script> tag in index.html and an entry in NBW_LANGUAGES below.
window.NBW_LANG = {};
window.NBW_LANGUAGES = [
  ["en", "English"],
  ["fr", "Français"],
  ["de", "Deutsch"],
  ["es", "Español"],
  ["it", "Italiano"],
  ["cs", "Čeština"],
  ["pt", "Português (Brasil)"],
  ["nl", "Nederlands"],
  ["ja", "日本語"],
];

(function () {
  let current = "en";

  // "auto" follows the system language; unknown languages fall back to English.
  function detect(pref) {
    if (pref && pref !== "auto" && NBW_LANG[pref]) return pref;
    const wanted = navigator.languages && navigator.languages.length ? navigator.languages : [navigator.language || "en"];
    for (const l of wanted) {
      const code = String(l).toLowerCase().split("-")[0];
      if (NBW_LANG[code]) return code;
    }
    return "en";
  }

  window.setLanguage = function (pref) {
    current = detect(pref);
    document.documentElement.lang = current;
  };
  window.getLanguage = function () {
    return current;
  };
  // t("key", { name: "value" }) replaces {name} in the translated text.
  window.t = function (key, vars) {
    const s = (NBW_LANG[current] && NBW_LANG[current][key]) ?? NBW_LANG.en[key] ?? key;
    return s.replace(/\{(\w+)\}/g, (_, k) => (vars && vars[k] !== undefined ? vars[k] : `{${k}}`));
  };
})();
