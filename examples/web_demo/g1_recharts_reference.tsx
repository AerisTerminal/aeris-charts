import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  Brush,
  CartesianGrid,
  Cell,
  ComposedChart,
  ErrorBar,
  Funnel,
  FunnelChart,
  LabelList,
  Legend,
  Line,
  LineChart,
  Pie,
  PieChart,
  PolarAngleAxis,
  PolarGrid,
  PolarRadiusAxis,
  Radar,
  RadarChart,
  RadialBar,
  RadialBarChart,
  ReferenceArea,
  ReferenceLine,
  ResponsiveContainer,
  Sankey,
  Scatter,
  ScatterChart,
  SunburstChart,
  Tooltip,
  Treemap,
  XAxis,
  YAxis,
  ZAxis,
} from "recharts";
import matrix from "./fixtures/g1/recharts_3_10_1/matrix.json";

const rows = [
  { name: "Jan", value: 12, alternate: 8, size: 90 },
  { name: "Feb", value: 18, alternate: 11, size: 140 },
  { name: "Mar", value: 14, alternate: 16, size: 110 },
  { name: "Apr", value: 24, alternate: 19, size: 180 },
];

const flow = {
  nodes: [{ name: "Visit" }, { name: "Trial" }, { name: "Paid" }],
  links: [{ source: 0, target: 1, value: 18 }, { source: 1, target: 2, value: 11 }],
};
const panel = { width: 350, height: 185 };

function Evidence({ api, children }: { api: string; children: React.ReactNode }) {
  return <div data-reference-api={api}>{children}</div>;
}

function StandardLine({ syncId, brush = false, animation = false, data = rows }: {
  syncId?: string;
  brush?: boolean;
  animation?: boolean;
  data?: typeof rows;
}) {
  return <LineChart {...panel} data={data} syncId={syncId} accessibilityLayer>
    <CartesianGrid />
    <XAxis dataKey="name" />
    <YAxis />
    <Tooltip />
    <Legend />
    <Line
      dataKey="value"
      stroke="#4c8bf5"
      isAnimationActive={animation}
      animationDuration={animation ? 400 : undefined}
      animationEasing="ease-in-out"
    />
    <Line dataKey="alternate" stroke="#28b7a4" isAnimationActive={false} />
    {brush ? <Brush dataKey="name" height={24} /> : null}
  </LineChart>;
}

function AnimationScene() {
  const [data, setData] = useState(rows);
  return <>
    <button type="button" onClick={() => setData((current) =>
      current.map((row, index) => ({ ...row, value: row.value + [3, 7, 5, 9][index] })))}>
      Update animated line
    </button>
    <StandardLine animation data={data} />
  </>;
}

function TwoLineTick({ x, y, payload }: {
  x?: number; y?: number; payload?: { value: string };
}) {
  const [month, description] = (payload?.value ?? "").split(" ");
  return <text x={x} y={y} textAnchor="middle" fill="#666" fontSize={12}>
    <tspan x={x} dy="0.4em">{month}</tspan>
    <tspan x={x} dy="1.1em">{description}</tspan>
  </text>;
}

function PersistenceScene() {
  const [data, setData] = useState(rows);
  const [saved, setSaved] = useState("");
  return <>
    <Evidence api="host-state">
      <button type="button" onClick={() => setData((current) => [...current].reverse())}>
        Update host state
      </button>
    </Evidence>
    <Evidence api="DOM/SVG export">
      <button type="button" onClick={() => setSaved(JSON.stringify(data))}>
        Serialize state for export
      </button>
      <output>{saved}</output>
    </Evidence>
    <LineChart {...panel} data={data}>
      <XAxis dataKey="name" />
      <YAxis />
      <Line dataKey="value" stroke="#4c8bf5" isAnimationActive={false} />
    </LineChart>
  </>;
}

function MigrationScene() {
  const [composed, setComposed] = useState(false);
  return <>
    <Evidence api="package-root-import">
      <code>import {"{ ComposedChart, Line, Bar }"} from "recharts"</code>
    </Evidence>
    <Evidence api="migration-toggle">
      <button type="button" onClick={() => setComposed(true)}>Migrate to composed chart</button>
    </Evidence>
    {composed
      ? <ComposedChart {...panel} data={rows}>
          <XAxis dataKey="name" /><YAxis />
          <Bar dataKey="alternate" fill="#28b7a4" />
          <Line dataKey="value" stroke="#4c8bf5" />
        </ComposedChart>
      : <LineChart {...panel} data={rows}>
          <XAxis dataKey="name" /><YAxis /><Line dataKey="value" stroke="#4c8bf5" />
        </LineChart>}
  </>;
}

function ReferenceScene({ id }: { id: string }) {
  if (id === "standalone-composed") {
    return <>
      <Evidence api="LineChart"><StandardLine /></Evidence>
      <Evidence api="ComposedChart">
        <ComposedChart {...panel} data={rows}>
          <XAxis dataKey="name" /><YAxis />
          <Area dataKey="alternate" fill="#b7eadf" stroke="#28b7a4" />
          <Bar dataKey="value" fill="#8b6df6" />
          <Line dataKey="value" stroke="#315fc8" />
        </ComposedChart>
      </Evidence>
    </>;
  }
  if (id === "paths-points") {
    return <>
      <Evidence api="Line"><StandardLine /></Evidence>
      <Evidence api="Area">
        <AreaChart {...panel} data={rows}>
          <XAxis dataKey="name" /><YAxis />
          <Area dataKey="value" fill="#b7eadf" stroke="#067a67" />
        </AreaChart>
      </Evidence>
      <Evidence api="Line ErrorBar">
        <LineChart {...panel} data={[
          { name: "Jan", value: 12, error: 2 }, { name: "Feb", value: 18, error: 3 },
          { name: "Mar", value: 14, error: 1 }, { name: "Apr", value: 24, error: 4 },
        ]}>
          <XAxis dataKey="name" /><YAxis />
          <Line dataKey="value" stroke="#4c8bf5" isAnimationActive={false}>
            <ErrorBar dataKey="error" stroke="#e46f61" isAnimationActive={false} />
          </Line>
        </LineChart>
      </Evidence>
      <Evidence api="Area range">
        <AreaChart {...panel} data={[
          { name: "Jan", range: [8, 12] }, { name: "Feb", range: [11, 18] },
          { name: "Mar", range: [10, 14] }, { name: "Apr", range: [19, 24] },
        ]}>
          <XAxis dataKey="name" /><YAxis />
          <Area dataKey="range" fill="#b7eadf" stroke="#067a67" isAnimationActive={false} />
        </AreaChart>
      </Evidence>
      <Evidence api="Line step gaps">
        <LineChart {...panel} data={[
          { name: "Jan", value: 12 }, { name: "Feb", value: 18 },
          { name: "Mar", value: null }, { name: "Apr", value: 14 },
          { name: "May", value: 24 },
        ]}>
          <XAxis dataKey="name" /><YAxis />
          <Line dataKey="value" type="step" stroke="#e46f61" dot={false} isAnimationActive={false} />
          <Line dataKey="value" type="step" connectNulls stroke="#4c8bf5" dot={false} isAnimationActive={false} />
        </LineChart>
      </Evidence>
      <Evidence api="Scatter">
        <ScatterChart {...panel}>
          <XAxis dataKey="value" type="number" /><YAxis dataKey="alternate" type="number" />
          <Scatter data={rows} fill="#8b6df6" isAnimationActive={false} />
        </ScatterChart>
      </Evidence>
      <Evidence api="Bubble ZAxis">
        <ScatterChart {...panel}>
          <XAxis dataKey="value" type="number" /><YAxis dataKey="alternate" type="number" />
          <ZAxis dataKey="size" range={[100, 500]} />
          <Scatter data={rows} fill="#8b6df6" isAnimationActive={false} />
        </ScatterChart>
      </Evidence>
      <Evidence api="Scatter square">
        <ScatterChart {...panel}>
          <XAxis dataKey="value" type="number" /><YAxis dataKey="alternate" type="number" />
          <Scatter data={rows} shape="square" fill="#8b6df6" isAnimationActive={false} />
        </ScatterChart>
      </Evidence>
    </>;
  }
  if (id === "bars-stacks") {
    return <><Evidence api="Bar stackId"><BarChart {...panel} data={rows}>
      <CartesianGrid />
      <XAxis dataKey="name" />
      <YAxis />
      <Tooltip />
      <Legend />
      <Bar dataKey="value" stackId="s" fill="#4c8bf5" isAnimationActive={false} />
      <Bar dataKey="alternate" stackId="s" fill="#28b7a4" isAnimationActive={false} />
    </BarChart></Evidence>
      <Evidence api="Bar signed stackOffset"><BarChart {...panel} stackOffset="sign" data={[
        { name: "A", value: 12, alternate: -8 },
        { name: "B", value: -18, alternate: 11 },
        { name: "C", value: -14, alternate: -16 },
        { name: "D", value: 24, alternate: 19 },
      ]}>
        <XAxis dataKey="name" /><YAxis />
        <Bar dataKey="value" stackId="s" fill="#4c8bf5" isAnimationActive={false} />
        <Bar dataKey="alternate" stackId="s" fill="#28b7a4" isAnimationActive={false} />
      </BarChart></Evidence>
      <Evidence api="Bar expanded stackOffset"><BarChart {...panel} stackOffset="expand" data={rows}>
        <XAxis dataKey="name" /><YAxis />
        <Bar dataKey="value" stackId="s" fill="#4c8bf5" isAnimationActive={false} />
        <Bar dataKey="alternate" stackId="s" fill="#28b7a4" isAnimationActive={false} />
      </BarChart></Evidence>
      <Evidence api="Bar positive stackOffset"><BarChart {...panel} stackOffset="positive" data={[
        { name: "A", value: 12, alternate: -8 },
        { name: "B", value: -18, alternate: 11 },
        { name: "C", value: -14, alternate: -16 },
        { name: "D", value: 24, alternate: 19 },
      ]}>
        <XAxis dataKey="name" /><YAxis />
        <Bar dataKey="value" stackId="s" fill="#4c8bf5" isAnimationActive={false} />
        <Bar dataKey="alternate" stackId="s" fill="#28b7a4" isAnimationActive={false} />
      </BarChart></Evidence>
      <Evidence api="Bar cumulative stackOffset"><BarChart {...panel} stackOffset="none" data={[
        { name: "A", value: 12, alternate: -18 },
        { name: "B", value: -18, alternate: 11 },
        { name: "C", value: 6, alternate: 4 },
        { name: "D", value: 10, alternate: 5 },
      ]}>
        <XAxis dataKey="name" /><YAxis />
        <Bar dataKey="value" stackId="s" fill="#4c8bf5" isAnimationActive={false} />
        <Bar dataKey="alternate" stackId="s" fill="#28b7a4" isAnimationActive={false} />
      </BarChart></Evidence>
      <Evidence api="Bar silhouette stackOffset"><BarChart {...panel} stackOffset="silhouette" data={rows}>
        <XAxis dataKey="name" /><YAxis />
        <Bar dataKey="value" stackId="s" fill="#4c8bf5" isAnimationActive={false} />
        <Bar dataKey="alternate" stackId="s" fill="#28b7a4" isAnimationActive={false} />
      </BarChart></Evidence>
      <Evidence api="Bar wiggle stackOffset"><BarChart {...panel} stackOffset="wiggle" data={rows}>
        <XAxis dataKey="name" /><YAxis />
        <Bar dataKey="value" stackId="s" fill="#4c8bf5" isAnimationActive={false} />
        <Bar dataKey="alternate" stackId="s" fill="#28b7a4" isAnimationActive={false} />
      </BarChart></Evidence>
      <Evidence api="Bar mixed expanded stackOffset"><BarChart {...panel} stackOffset="expand" data={[
        { name: "A", value: 12, alternate: -8 },
        { name: "B", value: -18, alternate: 11 },
        { name: "C", value: -14, alternate: -16 },
        { name: "D", value: 24, alternate: 19 },
      ]}>
        <XAxis dataKey="name" /><YAxis />
        <Bar dataKey="value" stackId="s" fill="#4c8bf5" isAnimationActive={false} />
        <Bar dataKey="alternate" stackId="s" fill="#28b7a4" isAnimationActive={false} />
      </BarChart></Evidence>
    </>;
  }
  if (id === "box-heatmap") {
    return <>
      <Evidence api="Scatter shape composition">
        <ScatterChart {...panel}>
          <XAxis dataKey="value" type="number" /><YAxis dataKey="alternate" type="number" />
          <Scatter data={rows} fill="#8b6df6" isAnimationActive={false} />
        </ScatterChart>
      </Evidence>
      <Evidence api="Treemap color reference">
        <Treemap {...panel} data={rows} dataKey="value" nameKey="name" fill="#4c8bf5" />
      </Evidence>
    </>;
  }
  if (id === "scales-axes") {
    return <>
      <Evidence api="XAxis YAxis CartesianGrid"><StandardLine /></Evidence>
      <Evidence api="XAxis fixed height comparison">
        <LineChart {...panel} data={rows}>
          <XAxis dataKey="name" height={30} tick={{ fontSize: 22 }} label="Quarter" />
          <YAxis /><Line dataKey="value" stroke="#4c8bf5" isAnimationActive={false} />
        </LineChart>
      </Evidence>
      <Evidence api="XAxis automatic height">
        <LineChart {...panel} data={rows}>
          <XAxis dataKey="name" height="auto" tick={{ fontSize: 22 }} label="Quarter" />
          <YAxis /><Line dataKey="value" stroke="#4c8bf5" isAnimationActive={false} />
        </LineChart>
      </Evidence>
      <Evidence api="XAxis rotated long ticks">
        <LineChart {...panel} data={rows.map((row) => ({ ...row, name: `${row.name} revenue` }))}>
          <XAxis dataKey="name" height="auto" angle={-60} tick={{ fontSize: 12 }} />
          <YAxis /><Line dataKey="value" stroke="#4c8bf5" isAnimationActive={false} />
        </LineChart>
      </Evidence>
      <Evidence api="XAxis custom multiline tick">
        <LineChart {...panel} data={rows.map((row) => ({ ...row, name: `${row.name} Revenue` }))}>
          <XAxis dataKey="name" height={58} interval={0} tick={<TwoLineTick />} />
          <YAxis /><Line dataKey="value" stroke="#4c8bf5" isAnimationActive={false} />
        </LineChart>
      </Evidence>
      <Evidence api="default horizontal bars">
        <BarChart width={480} height={300} layout="vertical" data={[
          { name: "First", amount: 20 },
          { name: "Second", amount: 40 },
        ]}>
          <XAxis type="number" />
          <YAxis type="category" dataKey="name" />
          <Bar dataKey="amount" fill="#d02020" isAnimationActive={false} />
        </BarChart>
      </Evidence>
    </>;
  }
  if (id === "polar") {
    return <>
      <Evidence api="PieChart Pie">
        <PieChart {...panel}>
          <Pie data={rows} dataKey="value" nameKey="name" innerRadius={30} outerRadius={65}>
            {rows.map((row, index) =>
              <Cell key={row.name} fill={["#4c8bf5", "#28b7a4", "#8b6df6", "#e4a11b"][index]} />)}
          </Pie>
        </PieChart>
      </Evidence>
      <Evidence api="RadarChart Radar">
        <RadarChart {...panel} data={rows}>
          <PolarGrid /><PolarAngleAxis dataKey="name" /><PolarRadiusAxis />
          <Radar dataKey="value" fill="#4c8bf5" fillOpacity={0.35} stroke="#4c8bf5" />
        </RadarChart>
      </Evidence>
      <Evidence api="RadialBarChart RadialBar">
        <RadialBarChart {...panel} data={rows} innerRadius="20%" outerRadius="90%">
          <RadialBar dataKey="value" fill="#28b7a4" />
        </RadialBarChart>
      </Evidence>
    </>;
  }
  if (id === "hierarchy-flow") {
    const tree = [{ name: "All", value: 44, children: rows }];

    return <>
      <Evidence api="FunnelChart Funnel">
        <FunnelChart {...panel}><Funnel data={rows} dataKey="value" fill="#8b6df6" isAnimationActive={false} /></FunnelChart>
      </Evidence>
      <Evidence api="Treemap">
        <Treemap {...panel} data={tree} dataKey="value" nameKey="name" fill="#4c8bf5" />
      </Evidence>
      <Evidence api="Sankey">
        <Sankey width={350} height={185} data={flow} node={{ fill: "#4c8bf5" }} link={{ stroke: "#8b6df6" }} />
      </Evidence>
      <Evidence api="SunburstChart">
        <SunburstChart width={350} height={185} data={tree[0]} dataKey="value" nameKey="name" fill="#28b7a4" />
      </Evidence>
    </>;
  }
  if (id === "legend-tooltip") {
    return <>
      <Evidence api="Legend Tooltip"><StandardLine /></Evidence>
      <Evidence api="Legend position top offset">
        <LineChart {...panel} data={rows}>
          <XAxis dataKey="name" /><YAxis />
          <Legend position="top" offset={8} />
          <Line dataKey="value" stroke="#4c8bf5" isAnimationActive={false} />
        </LineChart>
      </Evidence>
      <Evidence api="Legend custom position">
        <LineChart {...panel} data={rows}>
          <XAxis dataKey="name" /><YAxis />
          <Legend position={{ x: 120, y: 100 }} offset={80} />
          <Line dataKey="value" stroke="#4c8bf5" isAnimationActive={false} />
        </LineChart>
      </Evidence>
    </>;
  }
  if (id === "references-labels") {
    return <>
    <Evidence api="ReferenceLine ReferenceArea LabelList CartesianGrid">
      <AreaChart {...panel} data={rows}>
        <CartesianGrid /><XAxis dataKey="name" /><YAxis />
        <ReferenceArea x1="Feb" x2="Mar" fill="#fbe2a6" />
        <ReferenceLine y={16} stroke="#e46f61" />
        <Area dataKey="value" fill="#b7eadf" stroke="#067a67">
          <LabelList dataKey="value" />
        </Area>
      </AreaChart>
    </Evidence>
    <Evidence api="ReferenceArea optional bounds">
      <AreaChart {...panel} data={rows}>
        <XAxis dataKey="name" /><YAxis />
        <ReferenceArea y1={8} y2={16} fill="#b7eadf" />
        <ReferenceArea x1="Feb" fill="#fbe2a6" />
        <Area dataKey="value" fill="#ddeaf6" stroke="#4c8bf5" isAnimationActive={false} />
      </AreaChart>
    </Evidence>
    </>;
  }
  if (id === "brush-selection-sync") {
    return <>
      <Evidence api="Brush"><StandardLine syncId="g1" brush /></Evidence>
      <Evidence api="syncId"><StandardLine syncId="g1" /></Evidence>
    </>;
  }
  if (id === "responsive-layout") {
    return <>
      <Evidence api="ResponsiveContainer">
        <div data-resize-host style={{ width: 700, height: 350 }}>
          <ResponsiveContainer width="100%" height="100%"><StandardLine /></ResponsiveContainer>
        </div>
      </Evidence>
      <Evidence api="ResponsiveContainer aspect">
        <div data-aspect-host style={{ width: 600 }}>
          <ResponsiveContainer width="100%" aspect={2}><StandardLine /></ResponsiveContainer>
        </div>
      </Evidence>
    </>;
  }
  if (id === "react-authoring") {
    return <Evidence api="declarative chart axis series children"><StandardLine /></Evidence>;
  }
  if (id === "customization") {
    return <Evidence api="Cell formatter style props">
      <BarChart {...panel} data={rows}>
        <XAxis dataKey="name" tick={{ fill: "#315fc8", fontWeight: 600 }} />
        <YAxis /><Tooltip formatter={(value) => [`${value} units`, "Localized value"]} />
        <Bar dataKey="value" radius={[6, 6, 0, 0]} isAnimationActive={false}>
          {rows.map((row, index) =>
            <Cell key={row.name} fill={["#4c8bf5", "#28b7a4", "#8b6df6", "#e4a11b"][index]} />)}
        </Bar>
      </BarChart>
    </Evidence>;
  }
  if (id === "animation") {
    return <Evidence api="isAnimationActive animationDuration animationEasing">
      <AnimationScene />
    </Evidence>;
  }
  if (id === "accessibility-localization") {
    return <>
      <Evidence api="Sankey title desc focus">
        <Sankey width={350} height={185} data={flow} title="Flow comparison" desc="Value moving from source to destination" node={{ fill: "#4c8bf5" }} link={{ stroke: "#8b6df6" }} />
      </Evidence>
      <Evidence api="accessibilityLayer host formatters">
        <LineChart {...panel} data={rows.map((row) => ({ ...row, value: row.value * 100 }))} accessibilityLayer>
          <XAxis dataKey="name" /><YAxis domain={[0, 2400]} ticks={[0, 600, 1200, 1800, 2400]}
            tickFormatter={(value) => new Intl.NumberFormat("fr-FR").format(value)} />
          <Line dataKey="value" stroke="#4c8bf5" isAnimationActive={false} />
        </LineChart>
      </Evidence>
    </>;
  }
  if (id === "persistence-export-recovery") return <PersistenceScene />;
  if (id === "packaging-migration") {
    return <MigrationScene />;
  }
  throw new Error(`missing reference fixture for ${id}`);
}

function App() {
  const requested = new URLSearchParams(location.search).get("case");
  const initial = matrix.rows.find((row) => row.id === requested)?.id ?? matrix.rows[0].id;
  const [active, setActive] = useState(initial);
  const row = matrix.rows.find((candidate) => candidate.id === active)!;
  useEffect(() => {
    document.body.dataset.ready = "true";
  }, []);
  return <main>
    <header>
      <p>Private comparison fixture, not shipped in the Aeris package</p>
      <h1>Recharts {matrix.reference.version}</h1>
      <code>{matrix.reference.sourceRevision}</code>
    </header>
    <nav aria-label="Recharts capability fixtures">
      {matrix.rows.map((candidate) =>
        <button
          key={candidate.id}
          type="button"
          aria-pressed={candidate.id === active}
          onClick={() => setActive(candidate.id)}
        >
          {candidate.capability}
        </button>)}
    </nav>
    <section aria-label={row.capability} data-case={row.id}>
      <h2>{row.capability}</h2>
      <p>{row.referenceApi}</p>
      <div className="chart"><ReferenceScene id={row.id} /></div>
    </section>
  </main>;
}

createRoot(document.getElementById("root")!).render(<App />);
