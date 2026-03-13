/**
 * Parse the `// @name: <value>` front-matter from a Rhai script source.
 * Returns the name string, or '' if not found.
 */
export function parseFrontMatterName(source: string): string {
  for (const line of source.split('\n')) {
    const trimmed = line.trim();
    if (!trimmed.startsWith('//')) {
      if (trimmed === '') continue;
      break;
    }
    const body = trimmed.replace(/^\/\/\s*/, '');
    if (body.startsWith('@name:')) {
      return body.slice('@name:'.length).trim();
    }
  }
  return '';
}
