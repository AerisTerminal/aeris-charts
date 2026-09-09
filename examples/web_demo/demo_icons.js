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
const compact_layout = window.matchMedia("(max-width: 820px)");
const control_jump = document.getElementById("control_jump");
const section_buttons = [...document.querySelectorAll("#tool_rail [data-scroll-target]")];

new ResizeObserver(([entry]) => {
  document.documentElement.style.setProperty("--demo-header-height", `${entry.target.getBoundingClientRect().height}px`);
}).observe(document.getElementById("bar"));

function set_inspector(open) {
  inspector.dataset.open = String(open);
  document.getElementById("workspace").dataset.inspectorOpen = String(open);
  backdrop.dataset.open = String(open);
  inspector_toggle.setAttribute("aria-expanded", String(open));
  inspector.inert = !open;
  document.getElementById("chart_wrap").inert = open && compact_layout.matches;
}

inspector_toggle.setAttribute("aria-controls", "inspector");
inspector_toggle.addEventListener("click", () => {
  const open = inspector.dataset.open !== "true";
  set_inspector(open);
  if (open && compact_layout.matches) control_jump.focus();
});
backdrop.addEventListener("click", () => {
  set_inspector(false);
  inspector_toggle.focus();
});
set_inspector(!compact_layout.matches);
compact_layout.addEventListener("change", (event) => set_inspector(!event.matches));

function jump_to_section(id) {
  set_inspector(true);
  // Section navigation is frequent; immediate positioning keeps the chart stable.
  document.getElementById(id)?.scrollIntoView({ behavior: "instant", block: "start" });
  control_jump.value = id;
  for (const peer of section_buttons) peer.setAttribute("aria-current", String(peer.dataset.scrollTarget === id));
}

control_jump.addEventListener("change", () => jump_to_section(control_jump.value));
for (const button of section_buttons) {
  const label = document.createElement("span");
  label.textContent = button.getAttribute("aria-label");
  button.appendChild(label);
  button.addEventListener("click", () => {
    jump_to_section(button.dataset.scrollTarget);
  });
}

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && compact_layout.matches && inspector.dataset.open === "true") {
    set_inspector(false);
    inspector_toggle.focus();
    event.preventDefault();
  }
});

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
