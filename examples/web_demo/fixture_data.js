/** Generate the deterministic OHLC fixture shared by Aeris, reference, and the native renderer. */
export function generate_fixture_data(fixture) {
  const out = [];
  let seed = fixture.seed;
  // Math.imul gives the same wrapping-u32 LCG sequence as the Rust fixture generator.
  const random = () => (seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 0xffffffff;
  let price = fixture.start_price;
  const hour = 3600;
  const start = fixture.end_time - (fixture.bar_count - 1) * hour;
  for (let index = 0; index < fixture.bar_count; index += 1) {
    const open = price;
    const close = Math.max(1, open + (random() - 0.5) * fixture.close_span);
    out.push({
      time: start + index * hour,
      open,
      high: Math.max(open, close) + random() * fixture.wick_span,
      low: Math.min(open, close) - random() * fixture.wick_span,
      close,
    });
    price = close;
  }
  return out;
}
