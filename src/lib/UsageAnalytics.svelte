<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import {
    getBankedResetMarker,
    getRangeAnalytics,
    onAnalyticsResetWarning,
    setBankedResetMarker,
  } from './backend';
  import {
    analyticsFromDailyHistory,
    analyticsReportToUsagePeriod,
    availableTimeZones,
    comparePremiumSeats,
    dateTimeInZone,
    detectedTimeZone,
    formatInZone,
    mergeAnalyticsReports,
    rangeFromZonedLocal,
    relativeMoment,
    weeklyStart,
    zonedDateTimeToIso,
    type AnalyticsReport,
  } from './analytics';
  import type { ProviderSnapshot, UsageViewState } from './types';
  import type { CustomSpendSelection } from './totalSpend';

  interface ProviderOption {
    id: string;
    name: string;
  }
  interface ProviderResult {
    id: string;
    name: string;
    precision: 'event' | 'daily' | 'unavailable';
    report: AnalyticsReport | null;
    note: string;
  }
  interface PlanConfig {
    id: 'codex' | 'claude' | 'cursor';
    name: string;
    monthlyPrice: number;
    annualPrice: number | null;
    note: string;
  }

  let {
    viewState,
    providers,
    open = $bindable(false),
    onApply = () => {},
  }: {
    viewState: UsageViewState;
    providers: ProviderOption[];
    open?: boolean;
    onApply?: (selection: CustomSpendSelection) => void;
  } = $props();

  const planConfigs: PlanConfig[] = [
    {
      id: 'codex',
      name: 'ChatGPT Business Premium 5×',
      monthlyPrice: 125,
      annualPrice: 100,
      note: 'Business seat; US$125 monthly or US$100 monthly equivalent with annual billing.',
    },
    {
      id: 'cursor',
      name: 'Cursor Teams Premium 5×',
      monthlyPrice: 120,
      annualPrice: null,
      note: 'Teams Premium seat; Cursor history is available at daily precision.',
    },
    {
      id: 'claude',
      name: 'Claude Max 5×',
      monthlyPrice: 100,
      annualPrice: null,
      note: 'Individual Max 5× plan; this is not a Claude Team seat.',
    },
  ];
  const timeZones = availableTimeZones();
  const detectedZone = detectedTimeZone();
  const savedZone =
    typeof localStorage === 'undefined'
      ? null
      : localStorage.getItem('openquota.analytics.timeZone');
  const initialZone = savedZone && timeZones.includes(savedZone) ? savedZone : detectedZone;
  const savedCadence =
    typeof localStorage === 'undefined'
      ? null
      : localStorage.getItem('openquota.analytics.premiumCadence');

  let mode = $state('custom');
  let provider = $state('all');
  let timeZone = $state(initialZone);
  let start = $state(dateTimeInZone(new Date(Date.now() - 7 * 86400000), initialZone));
  let end = $state(dateTimeInZone(new Date(), initialZone));
  let manualReset = $state('');
  let billingCadence = $state<'monthly' | 'annual'>(
    savedCadence === 'annual' ? 'annual' : 'monthly',
  );
  let additionalApiCosts = $state<Record<string, number>>({ codex: 0, claude: 0, cursor: 0 });
  let marker = $state<string | null>(null);
  let busy = $state(false);
  let error = $state('');
  let resetWarning = $state('');
  let results = $state<ProviderResult[]>([]);
  let report = $state<AnalyticsReport | null>(null);
  let interval = $state('');
  let provenance = $state('');
  let reportFrom = $state('');
  let reportTo = $state('');
  let requestId = 0;

  const selectedProviders = $derived(
    provider === 'all' ? providers : providers.filter((option) => option.id === provider),
  );
  const weeklyBoundary = $derived(
    provider === 'all' ? null : weeklyStart(viewState, Date.now(), provider),
  );
  const visiblePlans = $derived(
    planConfigs.filter((plan) =>
      results.some((result) => result.id === plan.id && result.report !== null),
    ),
  );

  onDestroy(() => {
    requestId += 1;
  });
  onMount(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void Promise.resolve()
      .then(() =>
        onAnalyticsResetWarning((message) => {
          resetWarning = `${message} Enter the reset date manually; the saved marker may be stale.`;
          clearReport();
        }),
      )
      .then((unlisten) => {
        if (disposed) unlisten();
        else stop = unlisten;
      })
      .catch(() => {
        /* Desktop event unavailable in unit tests. */
      });
    return () => {
      disposed = true;
      stop?.();
    };
  });

  const usd = (value: number) =>
    new Intl.NumberFormat('en-US', {
      style: 'currency',
      currency: 'USD',
      maximumFractionDigits: 4,
    }).format(value);
  function family(id: string) {
    return id.split('@', 1)[0];
  }
  function clearReport() {
    results = [];
    report = null;
    error = '';
  }
  function close() {
    open = false;
  }
  function providerSnapshots(providerId: string): ProviderSnapshot[] {
    return Object.entries(viewState.providers)
      .filter(([id, state]) => family(id) === providerId && state.snapshot)
      .map(([, state]) => state.snapshot as ProviderSnapshot);
  }
  function providerStatus(option: ProviderOption) {
    const snapshots = providerSnapshots(option.id);
    const refreshedAt = snapshots
      .map((snapshot) => snapshot.refreshedAt)
      .filter((value) => Number.isFinite(Date.parse(value)))
      .sort((left, right) => Date.parse(right) - Date.parse(left))[0];
    const resetAt = snapshots
      .flatMap((snapshot) => snapshot.quotas)
      .map((quota) => quota.resetsAt)
      .filter((value): value is string => value !== null && Date.parse(value) > Date.now())
      .sort((left, right) => Date.parse(left) - Date.parse(right))[0];
    return { refreshedAt, resetAt };
  }
  function providerChanged() {
    if (provider === 'all' || !weeklyStart(viewState, Date.now(), provider)) mode = 'custom';
    if (mode === 'banked' && !['codex', 'claude'].includes(provider)) mode = 'custom';
    clearReport();
  }
  function selectTimeZone(event: Event) {
    const next = (event.currentTarget as HTMLSelectElement).value;
    if (next === timeZone) return;
    try {
      start = dateTimeInZone(new Date(zonedDateTimeToIso(start, timeZone)), next);
      end = dateTimeInZone(new Date(zonedDateTimeToIso(end, timeZone)), next);
      if (manualReset)
        manualReset = dateTimeInZone(new Date(zonedDateTimeToIso(manualReset, timeZone)), next);
    } catch {
      start = dateTimeInZone(new Date(Date.now() - 7 * 86400000), next);
      end = dateTimeInZone(new Date(), next);
      manualReset = '';
    }
    timeZone = next;
    localStorage.setItem('openquota.analytics.timeZone', next);
    clearReport();
  }
  function dailyResult(option: ProviderOption, from: string, to: string): ProviderResult {
    const startWallTime = dateTimeInZone(new Date(from), timeZone);
    const endWallTime = dateTimeInZone(new Date(to), timeZone);
    const reports = providerSnapshots(option.id).flatMap((snapshot) => {
      const daily = analyticsFromDailyHistory(snapshot.usage, startWallTime, endWallTime);
      return daily ? [daily] : [];
    });
    if (!reports.length)
      return {
        ...option,
        precision: 'unavailable',
        report: null,
        note: 'No usable daily history is available for this range.',
      };
    return {
      ...option,
      precision: 'daily',
      report: mergeAnalyticsReports(reports),
      note: 'Provider daily history; hours within a day cannot be separated.',
    };
  }
  async function eventResult(
    option: ProviderOption,
    from: string,
    to: string,
  ): Promise<ProviderResult> {
    try {
      return {
        ...option,
        precision: 'event',
        report: await getRangeAnalytics(from, to, option.id),
        note: 'Local event-level logs; start inclusive and end exclusive.',
      };
    } catch (caught) {
      if (provider !== 'all') throw caught;
      return {
        ...option,
        precision: 'unavailable',
        report: null,
        note: String(caught instanceof Error ? caught.message : caught),
      };
    }
  }
  function comparisonFor(plan: PlanConfig) {
    const providerReport = results.find((result) => result.id === plan.id)?.report;
    if (!providerReport || !reportFrom || !reportTo) return null;
    const monthlyPrice =
      plan.id === 'codex' && billingCadence === 'annual' && plan.annualPrice
        ? plan.annualPrice
        : plan.monthlyPrice;
    return comparePremiumSeats({
      seatCount: 2,
      monthlyPricePerSeatUsd: monthlyPrice,
      additionalApiCostUsd: Number(additionalApiCosts[plan.id]) || 0,
      apiEquivalentUsd: providerReport.estimatedCostUsd,
      from: reportFrom,
      to: reportTo,
    });
  }

  async function calculate() {
    if (busy) return;
    const id = ++requestId;
    clearReport();
    busy = true;
    try {
      let from: string;
      let to: string;
      let source: string;
      if (mode === 'weekly') {
        const boundary = weeklyStart(viewState, Date.now(), provider);
        if (!boundary)
          throw new Error('The weekly window is unavailable or stale. Refresh provider usage.');
        from = boundary;
        to = new Date().toISOString();
        source = 'Inferred start: next live reset minus seven days.';
      } else if (mode === 'banked') {
        if (resetWarning && !manualReset) throw new Error(resetWarning);
        marker = await getBankedResetMarker(provider);
        if (manualReset) {
          from = zonedDateTimeToIso(manualReset, timeZone);
          await setBankedResetMarker(provider, from);
          source = `Reset date saved in ${timeZone}; not verified by the provider.`;
        } else if (marker) {
          from = marker;
          source = `Saved ${provider === 'claude' ? 'Claude' : 'Codex'} reset from this local installation.`;
        } else throw new Error('No reset is recorded here. Enter the last banked reset date.');
        to = new Date().toISOString();
      } else {
        [from, to] = rangeFromZonedLocal(start, end, timeZone);
        source = `Custom range in ${timeZone}; start inclusive and end exclusive.`;
      }
      const loaded = await Promise.all(
        selectedProviders.map((option) =>
          ['codex', 'claude'].includes(option.id)
            ? eventResult(option, from, to)
            : Promise.resolve(dailyResult(option, from, to)),
        ),
      );
      if (id !== requestId) return;
      results = loaded;
      const available = loaded.flatMap((result) => (result.report ? [result.report] : []));
      report = available.length ? mergeAnalyticsReports(available) : null;
      if (!report) throw new Error('No usable historical data is available for this range.');
      reportFrom = from;
      reportTo = to;
      interval = `${formatInZone(from, timeZone)} → ${formatInZone(to, timeZone)} (${timeZone})`;
      provenance = source;
      onApply({
        from,
        to,
        timeZone,
        label: interval,
        providers: loaded.flatMap((result) => {
          if (!result.report) return [];
          return [
            {
              id: result.id,
              period: analyticsReportToUsagePeriod(result.report),
            },
          ];
        }),
      });
    } catch (caught) {
      if (id === requestId) error = String(caught instanceof Error ? caught.message : caught);
    } finally {
      if (id === requestId) busy = false;
    }
  }
</script>

<svelte:window
  onkeydown={(event) => {
    if (open && event.key === 'Escape') close();
  }}
/>

{#if open}
  <div
    class="analytics-overlay"
    role="presentation"
    onclick={(event) => {
      if (event.currentTarget === event.target) close();
    }}
  >
    <div class="analytics" role="dialog" aria-modal="true" aria-labelledby="analytics-title">
      <header>
        <div>
          <h3 id="analytics-title">Custom range</h3>
          <small>All providers are selected by default.</small>
        </div>
        <button class="close" type="button" aria-label="Close" onclick={close}>×</button>
      </header>
      <div class="body">
        <label
          >Provider
          <select bind:value={provider} disabled={busy} onchange={providerChanged}>
            <option value="all">All providers</option>
            {#each providers as option (option.id)}<option value={option.id}>{option.name}</option
              >{/each}
          </select>
        </label>
        <label
          >Usage since
          <select bind:value={mode} disabled={busy} onchange={clearReport}>
            <option value="custom">Custom range</option>
            {#if weeklyBoundary}<option value="weekly">Last weekly reset</option>{/if}
            {#if provider === 'codex' || provider === 'claude'}<option value="banked"
                >Last banked {provider === 'claude' ? 'Claude' : 'Codex'} reset</option
              >{/if}
          </select>
        </label>
        <label
          >Local time zone
          <select value={timeZone} disabled={busy} onchange={selectTimeZone}>
            {#each timeZones as zone (zone)}<option value={zone}
                >{zone}{zone === detectedZone ? ' · detected' : ''}</option
              >{/each}
          </select>
        </label>
        {#if mode === 'custom'}
          <div class="date-grid">
            <label
              >Start · {timeZone}<input
                type="datetime-local"
                bind:value={start}
                disabled={busy}
              /></label
            >
            <label
              >End · {timeZone}<input
                type="datetime-local"
                bind:value={end}
                disabled={busy}
              /></label
            >
          </div>
        {:else if mode === 'banked'}
          <label
            >Earlier reset or reset completed elsewhere (optional)<input
              type="datetime-local"
              bind:value={manualReset}
              disabled={busy}
            /></label
          >
        {/if}

        <section class="provider-status" aria-labelledby="provider-status-title">
          <h4 id="provider-status-title">Local times</h4>
          {#each selectedProviders as option (option.id)}
            {@const status = providerStatus(option)}
            <div class="status-row">
              <strong>{option.name}</strong>
              <span
                >Reset: {status.resetAt
                  ? `${relativeMoment(status.resetAt, Date.now(), true)} · ${formatInZone(status.resetAt, timeZone)}`
                  : 'unavailable'}</span
              >
              <span
                >Updated: {status.refreshedAt
                  ? `${relativeMoment(status.refreshedAt, Date.now(), false)} · ${formatInZone(status.refreshedAt, timeZone)}`
                  : 'unavailable'}</span
              >
            </div>
          {/each}
        </section>

        <button class="calculate" type="button" onclick={calculate} disabled={busy}
          >{busy ? 'Reading data…' : 'Calculate'}</button
        >
        {#if error}<p role="alert">{error}</p>{/if}
        {#if resetWarning}<p role="status">{resetWarning}</p>{/if}
        {#if report}
          <p class="interval">{interval}</p>
          <small>{provenance}</small>
          <section class="total" aria-labelledby="analytics-total-title">
            <h4 id="analytics-total-title">Observed total</h4>
            <dl>
              <div>
                <dt>Tokens</dt>
                <dd>{report.tokens.toLocaleString('en-US')}</dd>
              </div>
              <div>
                <dt>Estimated API equivalent</dt>
                <dd>{usd(report.estimatedCostUsd)}</dd>
              </div>
              <div>
                <dt>Measurable completed tasks</dt>
                <dd>{report.completedTasks}</dd>
              </div>
              <div>
                <dt>Average cost / task</dt>
                <dd>
                  {report.averageCostUsd === null ? 'Unavailable' : usd(report.averageCostUsd)}
                </dd>
              </div>
              <div>
                <dt>Average duration / task</dt>
                <dd>
                  {report.averageSeconds === null
                    ? 'Unavailable'
                    : `${(report.averageSeconds / 60).toFixed(1)} min`}
                </dd>
              </div>
            </dl>
          </section>

          <section class="provider-results" aria-labelledby="provider-results-title">
            <h4 id="provider-results-title">Provider details</h4>
            {#each results as result (result.id)}
              <article class:unavailable={result.report === null}>
                <div class="result-heading">
                  <strong>{result.name}</strong><span
                    >{result.precision === 'event'
                      ? 'Event precision'
                      : result.precision === 'daily'
                        ? 'Daily precision'
                        : 'Unavailable'}</span
                  >
                </div>
                {#if result.report}
                  <dl>
                    <div>
                      <dt>Tokens</dt>
                      <dd>{result.report.tokens.toLocaleString('en-US')}</dd>
                    </div>
                    <div>
                      <dt>API equivalent</dt>
                      <dd>{usd(result.report.estimatedCostUsd)}</dd>
                    </div>
                    <div>
                      <dt>Tasks</dt>
                      <dd>{result.report.completedTasks || '—'}</dd>
                    </div>
                    <div>
                      <dt>Cost / task</dt>
                      <dd>
                        {result.report.averageCostUsd === null
                          ? '—'
                          : usd(result.report.averageCostUsd)}
                      </dd>
                    </div>
                    <div>
                      <dt>Time / task</dt>
                      <dd>
                        {result.report.averageSeconds === null
                          ? '—'
                          : `${(result.report.averageSeconds / 60).toFixed(1)} min`}
                      </dd>
                    </div>
                  </dl>
                {/if}<small>{result.note}</small>
              </article>
            {/each}
          </section>

          {#if visiblePlans.length}
            <section class="comparisons" aria-labelledby="comparisons-title">
              <h4 id="comparisons-title">Two-person plans vs API</h4>
              {#each visiblePlans as plan (plan.id)}
                {@const comparison = comparisonFor(plan)}
                {#if comparison}
                  <article class="comparison">
                    <h4>{plan.name}</h4>
                    {#if plan.id === 'codex'}
                      <label
                        >Billing<select
                          bind:value={billingCadence}
                          onchange={() =>
                            localStorage.setItem(
                              'openquota.analytics.premiumCadence',
                              billingCadence,
                            )}
                          ><option value="monthly">Monthly · US$125 / seat</option><option
                            value="annual">Annual · US$100 monthly equivalent / seat</option
                          ></select
                        ></label
                      >
                    {/if}
                    <label
                      >Additional API spend actually billed (USD)<input
                        type="number"
                        min="0"
                        step="0.01"
                        value={additionalApiCosts[plan.id]}
                        oninput={(event) => {
                          additionalApiCosts[plan.id] = Number(
                            (event.currentTarget as HTMLInputElement).value,
                          );
                        }}
                      /></label
                    >
                    <dl>
                      <div>
                        <dt>2 seats · monthly cost</dt>
                        <dd>{usd(comparison.monthlySeatCostUsd)}</dd>
                      </div>
                      <div>
                        <dt>Range share</dt>
                        <dd>{usd(comparison.proratedSeatCostUsd)}</dd>
                      </div>
                      <div>
                        <dt>+ entered API spend</dt>
                        <dd>{usd(Number(additionalApiCosts[plan.id]) || 0)}</dd>
                      </div>
                      <div>
                        <dt>Combined cost</dt>
                        <dd>{usd(comparison.combinedCostUsd)}</dd>
                      </div>
                      <div>
                        <dt>Work · API equivalent</dt>
                        <dd>{usd(comparison.apiEquivalentUsd)}</dd>
                      </div>
                      <div>
                        <dt>Indicative difference</dt>
                        <dd class:positive={comparison.valueDifferenceUsd >= 0}>
                          {usd(comparison.valueDifferenceUsd)}
                        </dd>
                      </div>
                      <div>
                        <dt>Value / cost</dt>
                        <dd>
                          {comparison.valueRatio === null
                            ? 'Unavailable'
                            : `${comparison.valueRatio.toFixed(2)}×`}
                        </dd>
                      </div>
                    </dl>
                    <small>{plan.note} Local estimate, not a bill.</small>
                  </article>
                {/if}
              {/each}
            </section>
          {/if}
          {#if report.unpricedEvents || report.unreadableFiles || report.malformedLines}<p
              role="status"
            >
              Partial data: {report.unpricedEvents} unpriced events; {report.unreadableFiles}
              unreadable files; {report.malformedLines} malformed lines.
            </p>{/if}
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .analytics-overlay {
    position: fixed;
    z-index: 1000;
    inset: 0;
    display: grid;
    padding: 12px;
    overflow: auto;
    background: rgb(0 0 0 / 58%);
    place-items: start center;
  }
  .analytics {
    box-sizing: border-box;
    width: min(100%, 520px);
    max-height: calc(100vh - 24px);
    overflow: auto;
    border: 1px solid var(--border-color, #7775);
    border-radius: 12px;
    background: var(--tray, #202020);
    box-shadow: 0 18px 50px rgb(0 0 0 / 35%);
    color: var(--text, white);
    font-size: 12px;
  }
  header,
  .result-heading {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 10px;
  }
  header {
    position: sticky;
    z-index: 2;
    top: 0;
    padding: 12px;
    border-bottom: 1px solid #8884;
    background: var(--tray, #202020);
  }
  header h3,
  header small,
  h4,
  p,
  dl {
    margin: 0;
  }
  .close {
    width: 30px;
    flex: 0 0 30px;
    padding: 3px;
    font-size: 20px;
    line-height: 1;
  }
  .body {
    display: grid;
    min-width: 0;
    padding: 12px;
    gap: 10px;
  }
  label {
    display: grid;
    gap: 5px;
  }
  input,
  select,
  button {
    box-sizing: border-box;
    width: 100%;
    min-width: 0;
    padding: 7px;
    border: 1px solid #8886;
    border-radius: 6px;
    background: var(--surface, #7771);
    color: inherit;
    font: inherit;
  }
  select option {
    background: #222;
    color: white;
  }
  button {
    cursor: pointer;
    font-weight: 600;
  }
  button:disabled {
    opacity: 0.6;
    cursor: wait;
  }
  .calculate {
    background: var(--button-hover, #7773);
  }
  .date-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }
  .provider-status,
  .total,
  .provider-results,
  .comparisons,
  article {
    display: grid;
    gap: 8px;
  }
  .provider-status,
  .total,
  .provider-results,
  .comparisons {
    padding: 10px;
    border: 1px solid #8884;
    border-radius: 8px;
  }
  .status-row,
  article {
    padding: 8px;
    border-radius: 6px;
    background: #8881;
  }
  .status-row {
    display: grid;
    gap: 3px;
  }
  .status-row span,
  .result-heading span,
  small,
  .interval {
    color: var(--secondary, #aaa);
    font-size: 10px;
    line-height: 1.45;
    overflow-wrap: anywhere;
  }
  article.unavailable {
    opacity: 0.75;
  }
  dl div {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    padding: 3px 0;
  }
  dd {
    margin: 0;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .positive {
    color: #35a45b;
  }
  [role='alert'] {
    color: #df5656;
  }
  @media (max-width: 390px) {
    .date-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
