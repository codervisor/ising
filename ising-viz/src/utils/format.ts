export function shortenPath(path: string, maxLen: number = 30): string {
  if (path.length <= maxLen) return path;
  const parts = path.split("/");
  if (parts.length <= 2) return path.slice(-maxLen);
  const file = parts[parts.length - 1];
  const dir = parts[parts.length - 2];
  const short = `.../${dir}/${file}`;
  return short.length <= maxLen ? short : file.slice(-maxLen);
}

export function formatNumber(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
  return n.toString();
}

export function fileName(path: string): string {
  return path.split("/").pop() || path;
}
