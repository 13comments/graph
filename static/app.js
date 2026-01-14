const chart = klinecharts.init('chart');
chart.createIndicator('MA', false, { id: 'candle_pane' });
chart.createIndicator('EMA', false, { id: 'candle_pane' });
chart.createIndicator('RSI', false, { id: 'indicator_pane' });

const indicatorEl = document.getElementById('indicator');
const fibEl = document.getElementById('fib');
const fibStartInput = document.getElementById('fib-start');
const fibEndInput = document.getElementById('fib-end');
const drawLineButton = document.getElementById('draw-line');

function formatNumber(value) {
  if (value === null || value === undefined) {
    return '—';
  }
  return Number(value).toFixed(2);
}

async function loadCandles() {
  const response = await fetch('/api/candles');
  const candles = await response.json();
  const data = candles.map((candle) => ({
    timestamp: new Date(candle.timestamp).getTime(),
    open: candle.open,
    high: candle.high,
    low: candle.low,
    close: candle.close,
    volume: candle.volume,
  }));
  chart.applyNewData(data);
}

async function loadIndicators() {
  const response = await fetch('/api/indicators');
  const indicators = await response.json();
  const last = indicators[indicators.length - 1];
  if (!last) {
    indicatorEl.textContent = 'No indicator data found.';
    return;
  }
  indicatorEl.innerHTML = `
    <dl>
      <div>
        <dt>SMA (14)</dt>
        <dd>${formatNumber(last.sma_14)}</dd>
      </div>
      <div>
        <dt>EMA (14)</dt>
        <dd>${formatNumber(last.ema_14)}</dd>
      </div>
      <div>
        <dt>RSI (14)</dt>
        <dd>${formatNumber(last.rsi_14)}</dd>
      </div>
    </dl>
  `;
}

async function loadFib() {
  const params = new URLSearchParams();
  if (fibStartInput.value) {
    params.set('start', fibStartInput.value);
  }
  if (fibEndInput.value) {
    params.set('end', fibEndInput.value);
  }
  const queryString = params.toString();
  const response = await fetch(`/api/fib${queryString ? `?${queryString}` : ''}`);
  const data = await response.json();
  fibEl.innerHTML = `
    <p>Low: ${formatNumber(data.low)} | High: ${formatNumber(data.high)}</p>
    <ul>
      ${data.levels
        .map(
          (level) =>
            `<li>${level.ratio.toFixed(3)} → ${formatNumber(level.value)}</li>`
        )
        .join('')}
    </ul>
  `;
}

async function init() {
  try {
    await Promise.all([loadCandles(), loadIndicators(), loadFib()]);
  } catch (error) {
    console.error(error);
    indicatorEl.textContent = 'Failed to load data. Check the console for details.';
  }
}

fibEndInput.value = '2024-02-07 00:00:00';
fibStartInput.value = '2024-02-01 00:00:00';

const fibRefreshButton = document.getElementById('fib-refresh');
fibRefreshButton.addEventListener('click', loadFib);
drawLineButton.addEventListener('click', () => {
  chart.createOverlay({
    name: 'segment',
    totalStep: 2,
    lock: false,
  });
});

init();
