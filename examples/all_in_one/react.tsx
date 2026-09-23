import {
  FinancialSeries,
  GeneralPane,
  NucleusChart,
  type GeneralAxisSpec,
  type GeneralSeriesSpec,
} from "@axiusflowhq/financial/react";
import type { series_data } from "@axiusflowhq/financial";

const summaryAxes: readonly GeneralAxisSpec[] = [
  { id: "month", dimension: "x", position: "bottom", scale: "band" },
  { id: "revenue", dimension: "y", position: "left", scale: "linear" },
];

export interface MarketDashboardProps {
  candles: readonly series_data[];
  revenue: GeneralSeriesSpec["data"];
}

export function MarketDashboard({ candles, revenue }: MarketDashboardProps) {
  const summarySeries: readonly GeneralSeriesSpec[] = [{
    key: "revenue",
    kind: "column",
    options: { x_axis_id: "month", y_axis_id: "revenue", title: "Revenue" },
    data: revenue,
  }];

  return (
    <NucleusChart options={{ autoSize: true }} style={{ width: "100%", height: 560 }}>
      <FinancialSeries kind="candlestick" data={candles} />
      <GeneralPane
        options={{ horizontal_domain: { type: "category", scale: "band" } }}
        axes={summaryAxes}
        series={summarySeries}
      />
    </NucleusChart>
  );
}
