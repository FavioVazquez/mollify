/**
 * Typed contract for `mollify --format json`.
 *
 * Mirrors the Rust `mollify-types` crate (the single source of truth) and is
 * version-pinned to the installed CLI. Import it to parse Mollify's output:
 *
 *   import type { MollifyReport, Finding } from "mollify/types";
 *
 * Every command emits a `kind`-discriminated envelope; switch on `kind` and
 * iterate `findings`.
 */

/** Current schema version of the JSON contract (`schema_version` field). */
export type SchemaVersion = "0.1";

/** Confidence tier on every finding. Only `certain` findings are auto-fixed. */
export type Confidence = "certain" | "likely" | "uncertain";

/** Severity controls CI exit behavior (`error` fails CI by default). */
export type Severity = "error" | "warn" | "off";

/** Whether a finding was introduced by the current change or inherited. */
export type Attribution = "introduced" | "inherited";

/** The analysis areas a finding can belong to. */
export type Category =
  | "dead-code"
  | "duplication"
  | "circular-dependency"
  | "complexity"
  | "architecture"
  | "dependency-hygiene"
  | "type-health"
  | "security";

/** A 1-based source location; `column`/`end_line` omitted when not meaningful. */
export interface Location {
  path: string;
  line: number;
  column?: number;
  end_line?: number;
}

/** A proposed, machine-actionable remediation. */
export interface Action {
  /** e.g. `remove-symbol`, `remove-import`, `remove-dependency`. */
  type: string;
  description: string;
  /** True only when Mollify can apply this deterministically and safely. */
  auto_fixable: boolean;
  /** The inline comment that would suppress this finding instead of fixing it. */
  suppression_comment?: string;
}

/** A single piece of deterministic evidence — the atom of every report. */
export interface Finding {
  /** Stable cross-run id, `<rule>:<hex>`. */
  fingerprint: string;
  /** Machine rule id, e.g. `unused-export`, `circular-dependency`. */
  rule: string;
  category: Category;
  severity: Severity;
  confidence: Confidence;
  attribution?: Attribution;
  /** Human-readable explanation of the evidence. */
  reason: string;
  location: Location;
  actions?: Action[];
}

/** Aggregate counts, always present so CI can gate without scanning findings. */
export interface Summary {
  total: number;
  errors: number;
  warnings: number;
  files_analyzed: number;
  introduced?: number;
}

/** A report that is just a sorted list of findings plus a summary. */
export interface FindingsReportBody {
  schema_version: SchemaVersion;
  summary: Summary;
  findings: Finding[];
}

/** The full audit envelope: a quality score plus the findings. */
export interface AuditReportBody {
  schema_version: SchemaVersion;
  /** 0–100 health score (higher is better). */
  quality_score: number;
  summary: Summary;
  findings: Finding[];
}

/** Per-file code metrics (radon/wily-style). */
export interface FileMetrics {
  path: string;
  loc: number;
  sloc: number;
  comment_lines: number;
  blank_lines: number;
  functions: number;
  total_cyclomatic: number;
  max_cyclomatic: number;
  maintainability_index: number;
  mi_rank: string;
}

export interface MetricsTotals {
  files: number;
  loc: number;
  sloc: number;
  functions: number;
  mean_maintainability_index: number;
}

export interface MetricsReportBody {
  schema_version: SchemaVersion;
  files: FileMetrics[];
  totals: MetricsTotals;
}

/** The `kind`-discriminated output envelope. Switch on `kind`. */
export type MollifyReport =
  | ({ kind: "audit" } & AuditReportBody)
  | ({ kind: "dead-code" } & FindingsReportBody)
  | ({ kind: "deps" } & FindingsReportBody)
  | ({ kind: "arch" } & FindingsReportBody)
  | ({ kind: "complexity" } & FindingsReportBody)
  | ({ kind: "dupes" } & FindingsReportBody)
  | ({ kind: "types" } & FindingsReportBody)
  | ({ kind: "security" } & FindingsReportBody)
  | ({ kind: "coverage" } & FindingsReportBody)
  | ({ kind: "metrics" } & MetricsReportBody);

/** Convenience alias for the audit envelope. */
export type AuditReport = Extract<MollifyReport, { kind: "audit" }>;
