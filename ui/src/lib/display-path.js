/**
 * Remove Windows device-path syntax from text shown to a person.
 * @param {string} path
 */
export function displayPath(path) {
  if (path.startsWith("\\\\?\\UNC\\")) return `\\\\${path.slice(8)}`;
  if (path.startsWith("\\\\?\\")) return path.slice(4);
  return path;
}
