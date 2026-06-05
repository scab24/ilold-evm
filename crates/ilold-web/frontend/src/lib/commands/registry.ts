
export type CommandCategory =
  | 'Recent'
  | 'Suggestion'
  | 'Action'
  | 'Mode'
  | 'Scenario'
  | 'Function'
  | 'Contract'
  | 'Path';

export interface PathMatchMeta {
  contract: string;
  terminal: string;
  matches: { field: string; value: string }[];
}

export interface Command {
  id: string;
  label: string;
  detail?: string;
  category: CommandCategory;
  icon?: string;
  keywords?: string[];
  pathMeta?: PathMatchMeta;
  shortcut?: string;
  keepOpenOnRun?: boolean;
  run: () => void | Promise<void>;
}

export const CATEGORY_ORDER: readonly CommandCategory[] = [
  'Recent',
  'Suggestion',
  'Action',
  'Mode',
  'Scenario',
  'Function',
  'Contract',
  'Path',
];
