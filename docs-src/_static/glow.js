// Pointer light on cards and the previous/next tiles, as on buildforpeople.dev and the TigerOCR docs.
// Each tracked element gets a .glow-layer child; the pointer position is kept in --gx and --gy.
(() => {
  if (!matchMedia("(any-hover: hover)").matches || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const track = (el) => {
    el.classList.add("ocm-glow");
    const layer = document.createElement("span");
    layer.className = "glow-layer";
    layer.setAttribute("aria-hidden", "true");
    el.append(layer);
    el.addEventListener("pointermove", (e) => {
      const r = el.getBoundingClientRect();
      el.style.setProperty("--gx", `${((e.clientX - r.left) / r.width) * 100}%`);
      el.style.setProperty("--gy", `${((e.clientY - r.top) / r.height) * 100}%`);
    });
  };
  const start = () => document.querySelectorAll(".bd-article .sd-card, .prev-next-area a").forEach(track);
  document.readyState === "loading" ? document.addEventListener("DOMContentLoaded", start) : start();
})();
