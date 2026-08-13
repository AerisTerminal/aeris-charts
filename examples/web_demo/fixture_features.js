/** Shared feature fixtures consumed through each library's public API. */
export function marker_fixture(data) {
  const marker = (index, position, shape, color, text) => ({
    time: data[index].time,
    position,
    shape,
    color,
    text,
  });
  return [
    marker(840, "aboveBar", "arrowDown", "#f7525f", "SELL"),
    marker(880, "belowBar", "arrowUp", "#089981", "BUY"),
    marker(920, "inBar", "circle", "#7e57c2", "MID"),
    marker(960, "aboveBar", "square", "#2962ff", "NOTE"),
  ];
}

export function volume_fixture(data) {
  return data.map((bar) => ({
    time: bar.time,
    value: Math.round(500 + Math.abs(bar.close - bar.open) * 4000 + 300),
    color: bar.close >= bar.open ? "#08998180" : "#f7525f80",
  }));
}
