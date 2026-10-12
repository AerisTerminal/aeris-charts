import {
  AerisChart,
  FinancialSeries,
  GeneralAxis,
  GeneralBrushControls,
  GeneralDataLabels,
  GeneralLegend,
  GeneralPane,
  GeneralReference,
  GeneralTooltip,
  MappedGeneralSeries,
} from "@aeristerminal/aeris-charts/react";
import type { series_data } from "@aeristerminal/aeris-charts";

export interface RevenueRow {
  id: string;
  month: string;
  value: number;
}

const mapRevenue = (row: RevenueRow) => ({
  id: row.id,
  x: row.month,
  y: row.value,
  label: `${row.value}`,
});

export interface MarketDashboardProps {
  candles: readonly series_data[];
  revenue: readonly RevenueRow[];
}

/** A financial pane and composable general pane share one chart and ordered frame. */
export function MarketDashboard({ candles, revenue }: MarketDashboardProps) {
  return (
    <AerisChart options={{ autoSize: true }} style={{ width: "100%", height: 560 }}>
      <FinancialSeries kind="candlestick" data={candles} />
      <GeneralPane options={{ horizontal_domain: { type: "category", scale: "band" } }}>
        <GeneralAxis id="month" dimension="x" scale="band" position="bottom" />
        <GeneralAxis id="revenue" dimension="y" scale="linear" position="left" title="Revenue" />
        <MappedGeneralSeries seriesKey="revenue" kind="column"
          options={{ x_axis_id: "month", y_axis_id: "revenue", title: "Revenue" }}
          data={revenue} mapRow={mapRevenue} />
        <GeneralDataLabels seriesKey="revenue" />
        <GeneralReference referenceKey="target"
          options={{ kind: "line", axis_id: "revenue", value: 50, color: "#f59e0b" }} />
        <GeneralLegend />
        <GeneralTooltip />
        <GeneralBrushControls options={{ axis: "month" }} />
      </GeneralPane>
    </AerisChart>
  );
}
