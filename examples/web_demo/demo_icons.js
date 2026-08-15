import Activity01Icon from "./node_modules/@hugeicons/core-free-icons/dist/esm/Activity01Icon.js";
import Analytics01Icon from "./node_modules/@hugeicons/core-free-icons/dist/esm/Analytics01Icon.js";
import ChartCandlestickIcon from "./node_modules/@hugeicons/core-free-icons/dist/esm/ChartCandlestickIcon.js";
import ChartHistogramIcon from "./node_modules/@hugeicons/core-free-icons/dist/esm/ChartHistogramIcon.js";
import GridViewIcon from "./node_modules/@hugeicons/core-free-icons/dist/esm/GridViewIcon.js";
import MagicWand01Icon from "./node_modules/@hugeicons/core-free-icons/dist/esm/MagicWand01Icon.js";
import Moon02Icon from "./node_modules/@hugeicons/core-free-icons/dist/esm/Moon02Icon.js";
import PaintBrush01Icon from "./node_modules/@hugeicons/core-free-icons/dist/esm/PaintBrush01Icon.js";
import PanelRightIcon from "./node_modules/@hugeicons/core-free-icons/dist/esm/PanelRightIcon.js";
import Redo02Icon from "./node_modules/@hugeicons/core-free-icons/dist/esm/Redo02Icon.js";
import RefreshIcon from "./node_modules/@hugeicons/core-free-icons/dist/esm/RefreshIcon.js";
import Search01Icon from "./node_modules/@hugeicons/core-free-icons/dist/esm/Search01Icon.js";
import Settings02Icon from "./node_modules/@hugeicons/core-free-icons/dist/esm/Settings02Icon.js";
import Sun03Icon from "./node_modules/@hugeicons/core-free-icons/dist/esm/Sun03Icon.js";
import Undo02Icon from "./node_modules/@hugeicons/core-free-icons/dist/esm/Undo02Icon.js";

const icons = {
  activity: Activity01Icon,
  analysis: Analytics01Icon,
  candles: ChartCandlestickIcon,
  chart: ChartHistogramIcon,
  draw: PaintBrush01Icon,
  grid: GridViewIcon,
  lab: MagicWand01Icon,
  moon: Moon02Icon,
  panel: PanelRightIcon,
  redo: Redo02Icon,
  refresh: RefreshIcon,
  search: Search01Icon,
  settings: Settings02Icon,
  sun: Sun03Icon,
  undo: Undo02Icon,
};

const namespace = "http://www.w3.org/2000/svg";

function render_icon(target, definition) {
  const svg = document.createElementNS(namespace, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("fill", "none");
  svg.setAttribute("aria-hidden", "true");
  for (const [tag, attributes] of definition) {
    const node = document.createElementNS(namespace, tag);
    for (const [name, value] of Object.entries(attributes)) {
      if (name === "key") continue;
      node.setAttribute(name.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`), String(value));
    }
    svg.appendChild(node);
  }
  target.replaceChildren(svg);
}

export function hydrate_icons(root = document) {
  for (const target of root.querySelectorAll("[data-icon]")) {
    const definition = icons[target.dataset.icon];
    if (definition !== undefined && target.childElementCount === 0) render_icon(target, definition);
  }
}

hydrate_icons();

const inspector = document.getElementById("inspector");
const backdrop = document.getElementById("mobile_backdrop");
const inspector_toggle = document.getElementById("inspector_toggle");

function set_inspector(open) {
  inspector.dataset.open = String(open);
  document.getElementById("workspace").dataset.inspectorOpen = String(open);
  backdrop.dataset.open = String(open);
  inspector_toggle.setAttribute("aria-expanded", String(open));
}

inspector_toggle.addEventListener("click", () => set_inspector(inspector.dataset.open !== "true"));
backdrop.addEventListener("click", () => set_inspector(false));
const compact_layout = window.matchMedia("(max-width: 820px)");
set_inspector(!compact_layout.matches);
compact_layout.addEventListener("change", (event) => set_inspector(!event.matches));

for (const button of document.querySelectorAll("#tool_rail [data-scroll-target]")) {
  button.addEventListener("click", () => {
    set_inspector(true);
    document.getElementById(button.dataset.scrollTarget)?.scrollIntoView({ behavior: "smooth", block: "start" });
    for (const peer of document.querySelectorAll("#tool_rail [data-scroll-target]")) {
      peer.setAttribute("aria-current", String(peer === button));
    }
  });
}

const theme_toggle = document.getElementById("theme_toggle");

function sync_theme_button() {
  const dark = document.documentElement.dataset.theme !== "light";
  theme_toggle.setAttribute("aria-label", dark ? "Switch to light theme" : "Switch to dark theme");
}

theme_toggle.addEventListener("click", () => {
  const select = document.getElementById("theme_select");
  select.value = document.documentElement.dataset.theme === "light" ? "dark" : "light";
  select.dispatchEvent(new Event("change", { bubbles: true }));
});

new MutationObserver(sync_theme_button).observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
sync_theme_button();
