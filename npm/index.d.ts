/** A file of the agent's folder: its path relative to the folder, and its text. */
export interface File {
  path: string;
  content: string;
}

export interface Finding {
  organ: "eyes" | "ears" | "brain" | "hands" | "heart";
  check: "hidden" | "comment" | "order" | "actor" | "wide" | "pay";
  kind: string;
  file: string;
  /** 1-based; 0 when the finding belongs to the file as a whole. */
  line: number;
  says: string;
  genes: string[];
}

export interface Report {
  /** Files read. */
  read: number;
  /** Files left unread because their names say they hold keys. */
  skipped: string[];
  findings: Finding[];
}

/**
 * Scans an agent's folder, as the check-up at https://venin.io/checkup does, with no network.
 *
 * @param files JSON text of `{ files: File[] }`.
 * @returns JSON text of a `Report`, or of `{ error: string }` when the input is not that shape.
 */
export function scan(files: string): string;
