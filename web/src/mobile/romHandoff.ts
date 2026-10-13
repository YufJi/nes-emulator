/**
 * Hand-off of a locally picked ROM between the two phone routes.
 *
 * A `File` cannot travel in a URL, and the phone layout has no drag & drop, so
 * the dashboard picks the file and the player — which owns the emulator —
 * collects it when it mounts.
 */

let pending: File | null = null;

export function setPendingRom(file: File): void {
  pending = file;
}

/** Take the file, if any; a handed-over file is only ever consumed once. */
export function takePendingRom(): File | null {
  const file = pending;
  pending = null;
  return file;
}
