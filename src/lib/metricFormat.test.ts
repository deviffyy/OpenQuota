import { describe, expect, it } from 'vitest';
import { setUiLanguage, t } from './i18n';
import {
  formatMetricNumber,
  formatMetricValue,
  formatSpendValue,
  totalSpendRingCenter,
} from './metricFormat';

describe('shared metric formatting', () => {
  it('keeps row values compact and tooltip values exact', () => {
    expect(formatMetricNumber(2059.07, 'dollars', 'row')).toBe('$2.1K');
    expect(formatMetricNumber(2059.07, 'dollars', 'full')).toBe('$2,059.07');
    expect(formatMetricValue(1_506_025_363, 'count', 'row', 'tokens')).toBe('1.5B tokens');
    expect(formatMetricValue(1_506_025_363, 'count', 'full', 'tokens')).toBe(
      '1,506,025,363 tokens',
    );
  });

  it('formats total spend consistently across its surfaces', () => {
    expect(formatSpendValue(2059.07, 'cost')).toBe('$2.1K');
    expect(formatSpendValue(2059.07, 'cost', 'full')).toBe('$2,059.07');
    expect(totalSpendRingCenter(2059.07, 'cost')).toEqual({
      primary: '$2.1K',
      unit: 'dollars',
    });
    expect(totalSpendRingCenter(461_800_000, 'tokens')).toEqual({
      primary: '461.8',
      unit: 'million',
    });
  });

  it('uses the resolved UI locale for numbers, percentages, currency, and complete messages', () => {
    setUiLanguage('zh-CN');
    expect(formatMetricNumber(12345.6, 'count', 'full')).toBe('12,345.6');
    expect(formatMetricNumber(25, 'percent', 'row')).toBe('25%');
    expect(formatMetricNumber(12.5, 'dollars', 'full')).toContain('12.50');
    expect(t('tokensValue', { count: formatMetricNumber(12000, 'count', 'row') })).toBe(
      '1.2万 个令牌',
    );
    expect(t('resetsTodayAt', { time: '18:30' })).toBe('今天 18:30 重置');
    setUiLanguage('en');
  });
});
