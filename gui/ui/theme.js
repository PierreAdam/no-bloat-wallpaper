// Runs before first paint so a saved theme choice never flashes the wrong colors.
(function () {
  try {
    var t = localStorage.getItem("nbw-theme");
    if (t === "dark" || t === "light") document.documentElement.dataset.theme = t;
  } catch (e) {}
})();
