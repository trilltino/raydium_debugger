export function shortAddress(value: string, head = 6, tail = 6): string {
  if (value.length <= head + tail + 3) return value;
  return `${value.slice(0, head)}...${value.slice(-tail)}`;
}

export function number(value: number | null | undefined): string {
  if (value === null || value === undefined) return '-';
  return new Intl.NumberFormat().format(value);
}

export function exactNumber(value: string | number | null | undefined): string {
  if (value === null || value === undefined || value === '') return '-';
  if (typeof value === 'number') return number(value);
  const asNumber = Number(value);
  if (Number.isSafeInteger(asNumber)) return number(asNumber);
  return value;
}

export function lamports(value: string | number | null | undefined): string {
  const formatted = exactNumber(value);
  return formatted === '-' ? '-' : `${formatted} lamports`;
}

export function signed(value: string | null | undefined): string {
  if (!value) return '-';
  return value.startsWith('-') ? value : `+${value}`;
}

export function dateTime(ts: number | null): string {
  if (!ts) return '-';
  return new Date(ts * 1000).toLocaleString();
}
