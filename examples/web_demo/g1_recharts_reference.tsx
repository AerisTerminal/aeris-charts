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
} from "recharts";
import matrix from "./fixtures/g1/recharts-3.10.1/matrix.json";

const rows = [
  { name: "Jan", value: 12, alternate: 8, size: 90 },
  { name: "Feb", value: 18, alternate: 11, size: 140 },
  { name: "Mar", value: 14, alternate: 16, size: 110 },
  { name: "Apr", value: 24, alternate: 19, size: 180 },
];

const panel = { width: 350, height: 185 };

function Evidence({ api, children }: { api: string; children: React.ReactNode }) {
  return <div data-reference-api={api}>{children}</div>;
}

function StandardLine({ syncId, brush = false, animation = false }: {
  syncId?: string;
  brush?: boolean;
  animation?: boolean;
}) {
  return <LineChart {...panel} data={rows} syncId={syncId} accessibilityLayer>
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
      <Evidence api="Scatter">
        <ScatterChart {...panel}>
          <XAxis dataKey="value" type="number" /><YAxis dataKey="alternate" type="number" />
          <Scatter data={rows} fill="#8b6df6" />
        </ScatterChart>
      </Evidence>
    </>;
  }
  if (id === "bars-stacks") {
    return <Evidence api="Bar stackId"><BarChart {...panel} data={rows}>
      <CartesianGrid />
      <XAxis dataKey="name" />
      <YAxis />
      <Tooltip />
      <Legend />
      <Bar dataKey="value" stackId="s" fill="#4c8bf5" />
      <Bar dataKey="alternate" stackId="s" fill="#28b7a4" />
    </BarChart></Evidence>;
  }
  if (id === "box-heatmap") {
    return <>
      <Evidence api="Scatter shape composition">
        <ScatterChart {...panel}>
          <XAxis dataKey="value" type="number" /><YAxis dataKey="alternate" type="number" />
          <Scatter data={rows} fill="#8b6df6" />
        </ScatterChart>
      </Evidence>
      <Evidence api="Treemap color reference">
        <Treemap {...panel} data={rows} dataKey="value" nameKey="name" fill="#4c8bf5" />
      </Evidence>
    </>;
  }
  if (id === "scales-axes") {
    return <Evidence api="XAxis YAxis CartesianGrid">
      <StandardLine />
    </Evidence>;
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
    const flow = {
      nodes: [{ name: "Visit" }, { name: "Trial" }, { name: "Paid" }],
      links: [{ source: 0, target: 1, value: 18 }, { source: 1, target: 2, value: 11 }],
    };
    return <>
      <Evidence api="FunnelChart Funnel">
        <FunnelChart {...panel}><Funnel data={rows} dataKey="value" fill="#8b6df6" /></FunnelChart>
      </Evidence>
      <Evidence api="Treemap">
        <Treemap {...panel} data={tree} dataKey="value" nameKey="name" fill="#4c8bf5" />
      </Evidence>
      <Evidence api="Sankey">
        <Sankey width={350} height={185} data={flow} node={{ fill: "#4c8bf5" }} link={{ stroke: "#8b6df6" }} />
      </Evidence>
      <Evidence api="SunburstChart">
        <SunburstChart width={350} height={185} data={tree} dataKey="value" nameKey="name" fill="#28b7a4" />
      </Evidence>
    </>;
  }
  if (id === "legend-tooltip") {
    return <Evidence api="Legend Tooltip"><StandardLine /></Evidence>;
  }
  if (id === "references-labels") {
    return <Evidence api="ReferenceLine ReferenceArea LabelList CartesianGrid">
      <AreaChart {...panel} data={rows}>
        <CartesianGrid /><XAxis dataKey="name" /><YAxis />
        <ReferenceArea x1="Feb" x2="Mar" fill="#fbe2a6" />
        <ReferenceLine y={16} stroke="#e46f61" />
        <Area dataKey="value" fill="#b7eadf" stroke="#067a67">
          <LabelList dataKey="value" />
        </Area>
      </AreaChart>
    </Evidence>;
  }
  if (id === "brush-selection-sync") {
    return <>
      <Evidence api="Brush"><StandardLine syncId="g1" brush /></Evidence>
      <Evidence api="syncId"><StandardLine syncId="g1" /></Evidence>
    </>;
  }
  if (id === "responsive-layout") {
    return <Evidence api="ResponsiveContainer">
      <div style={{ width: 700, height: 350 }}>
        <ResponsiveContainer width="100%" height="100%"><StandardLine /></ResponsiveContainer>
      </div>
    </Evidence>;
  }
  if (id === "react-authoring") {
    return <Evidence api="declarative chart axis series children"><StandardLine /></Evidence>;
  }
  if (id === "customization") {
    return <Evidence api="Cell formatter style props">
      <BarChart {...panel} data={rows}>
        <XAxis dataKey="name" tick={{ fill: "#315fc8", fontWeight: 600 }} />
        <YAxis /><Tooltip formatter={(value) => [`${value} units`, "Localized value"]} />
        <Bar dataKey="value" radius={[6, 6, 0, 0]}>
          {rows.map((row, index) =>
            <Cell key={row.name} fill={["#4c8bf5", "#28b7a4", "#8b6df6", "#e4a11b"][index]} />)}
        </Bar>
      </BarChart>
    </Evidence>;
  }
  if (id === "animation") {
    return <Evidence api="isAnimationActive animationDuration animationEasing">
      <StandardLine animation />
    </Evidence>;
  }
  if (id === "accessibility-localization") {
    return <Evidence api="accessibilityLayer host formatters">
      <LineChart {...panel} data={rows} accessibilityLayer>
        <XAxis dataKey="name" /><YAxis tickFormatter={(value) => new Intl.NumberFormat("fr-FR").format(value)} />
        <Line dataKey="value" stroke="#4c8bf5" />
      </LineChart>
    </Evidence>;
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
