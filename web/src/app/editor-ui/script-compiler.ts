import ts from 'typescript';
import type { ScriptConfig } from './editor-engine.service';

export const DEFAULT_TRIGGER_SCRIPT_SOURCE = `function onTrigger(ctx: TriggerContext): void {
  ctx.showPopup(ctx.entity.name, \`Triggered via \${ctx.way}!\`);
}
`;

export interface ScriptTranspileResult {
  compiledJs: string;
  diagnostics: string[];
}

export function transpileTypeScriptToJs(source: string): ScriptTranspileResult {
  const result = ts.transpileModule(source, {
    fileName: 'trigger.ts',
    compilerOptions: {
      target: ts.ScriptTarget.ES2022,
      module: ts.ModuleKind.None,
      removeComments: false,
      strict: false,
    },
    reportDiagnostics: true,
  });

  const diagnostics = (result.diagnostics ?? []).map((diag) => {
    const msg = ts.flattenDiagnosticMessageText(diag.messageText, '\n');
    if (diag.file && typeof diag.start === 'number') {
      const { line, character } = diag.file.getLineAndCharacterOfPosition(diag.start);
      return `Line ${line + 1}:${character + 1}: ${msg}`;
    }
    return msg;
  });

  return {
    compiledJs: result.outputText,
    diagnostics,
  };
}

export function defaultScriptConfig(): ScriptConfig {
  const { compiledJs } = transpileTypeScriptToJs(DEFAULT_TRIGGER_SCRIPT_SOURCE);
  return {
    source: DEFAULT_TRIGGER_SCRIPT_SOURCE,
    compiled_js: compiledJs,
  };
}

export interface ScriptSnippet {
  id: string;
  label: string;
  source: string;
}

export const SCRIPT_SNIPPETS: readonly ScriptSnippet[] = [
  {
    id: 'popup',
    label: 'Show Popup',
    source: DEFAULT_TRIGGER_SCRIPT_SOURCE,
  },
  {
    id: 'transfer_area',
    label: 'Transfer Player to Area',
    source: `function onTrigger(ctx: TriggerContext): void {
  // Transfer the player into another loaded area at (x, z, yawDeg).
  ctx.player.moveToArea('main', 8.0, 8.0, 0);
}
`,
  },
  {
    id: 'inventory_teleport',
    label: 'Inventory Check & Teleport Target',
    source: `function onTrigger(ctx: TriggerContext): void {
  const hasClothes =
    ctx.player.hasItem('cryo_jumpsuit') && ctx.player.hasItem('cargo_pants');
  const hasRifle = ctx.player.hasItem('guard_rifle');
  if (!hasClothes || !hasRifle) {
    ctx.showPopup(
      ctx.entity.name,
      'Maybe I should check out the guard and the dead prisoner first',
      'warning',
    );
    return;
  }
  ctx.teleportToArea('main', 'CryoElevatorExit');
}
`,
  },
  {
    id: 'quest_gate',
    label: 'Quest Grant & Completion',
    source: `function onTrigger(ctx: TriggerContext): void {
  const status = ctx.player.questStatus('wake_protocol');
  if (status === 'none') {
    ctx.player.giveQuest('wake_protocol', 'Check the Exit Terminal');
    ctx.showPopup('New Objective', 'Reach the exit bulkhead terminal.', 'info');
  } else if (status === 'active') {
    ctx.player.completeQuest('wake_protocol');
    ctx.showPopup('Objective Complete', 'Bulkhead unlocked!', 'success');
  } else {
    ctx.showPopup(ctx.entity.name, 'All protocols verified.', 'success');
  }
}
`,
  },
  {
    id: 'counter',
    label: 'Activation Counter Property',
    source: `function onTrigger(ctx: TriggerContext): void {
  const uses = (ctx.entity.getInt('uses') ?? 0) + 1;
  ctx.entity.setInt('uses', uses);
  ctx.log(\`\${ctx.entity.name} activated \${uses} time(s)\`);
  ctx.showPopup(ctx.entity.name, \`Activated \${uses} time(s).\`, 'info');
}
`,
  },
];

function toUnionLiteral(ids: readonly string[], fallback: string): string {
  const unique = Array.from(new Set(ids.map((id) => id.trim()).filter(Boolean))).sort();
  if (unique.length === 0) {
    return fallback;
  }
  return `${unique.map((id) => JSON.stringify(id)).join(' | ')} | (string & {})`;
}

export function buildFaithFireTypeDefs(
  areaIds: readonly string[],
  templateIds: readonly string[],
  itemIds: readonly string[] = [],
): string {
  const areaUnion = toUnionLiteral(areaIds, 'string');
  const tplUnion = toUnionLiteral(templateIds, 'string');
  const itemUnion = toUnionLiteral(itemIds, 'string');

  return `// Faith & Fire Entity Scripting API (generated live from current project data)
type AreaId = ${areaUnion};
type TemplateId = ${tplUnion};
type ItemId = ${itemUnion};
type PopupTone = 'info' | 'success' | 'warning' | 'danger';
type TriggerWay = 'click' | 'walk_into';
type QuestStatus = 'none' | 'active' | 'completed';
type PropertyPrimitive = boolean | number | string | null;

interface InventoryEntryView {
  readonly itemId: string;
  readonly count: number;
  readonly equipped: boolean;
}

interface ScriptEntityApi {
  /** Runtime entity ID. */
  readonly id: number;
  /** Resolved entity display name. */
  readonly name: string;
  /** World coordinates [x, y, z]. */
  readonly pos: [number, number, number];
  /** Reads a property from this entity, or null if absent. */
  getProp(key: string): PropertyPrimitive;
  getBool(key: string): boolean | null;
  getInt(key: string): number | null;
  getFloat(key: string): number | null;
  getString(key: string): string | null;
  /** Sets or removes (null) a property on this entity. */
  setProp(key: string, value: PropertyPrimitive): void;
  setInt(key: string, value: number): void;
  setFloat(key: string, value: number): void;
  /** Returns true if this entity carries at least minCount (default 1) of itemId. */
  hasItem(itemId: ItemId, minCount?: number): boolean;
  /** Returns total quantity of itemId in this entity's inventory. */
  itemCount(itemId: ItemId): number;
  /** Returns true if itemId is currently equipped on this entity. */
  isEquipped(itemId: ItemId): boolean;
  /** Returns all item entries in this entity's inventory. */
  items(): InventoryEntryView[];
  /** Despawns this entity instance from the active simulation. */
  despawn(): void;
}

interface ScriptPlayerApi {
  /** Runtime player entity ID. */
  readonly id: number;
  /** Current world coordinates [x, y, z]. */
  readonly pos: [number, number, number];
  getProp(key: string): PropertyPrimitive;
  getBool(key: string): boolean | null;
  getInt(key: string): number | null;
  getFloat(key: string): number | null;
  getString(key: string): string | null;
  setProp(key: string, value: PropertyPrimitive): void;
  setInt(key: string, value: number): void;
  setFloat(key: string, value: number): void;
  /** Returns true if the player has at least minCount (default 1) of itemId. */
  hasItem(itemId: ItemId, minCount?: number): boolean;
  /** Returns total quantity of itemId in the player's inventory. */
  itemCount(itemId: ItemId): number;
  /** Returns true if itemId is currently equipped on the player. */
  isEquipped(itemId: ItemId): boolean;
  /** Returns all item entries in the player's inventory. */
  items(): InventoryEntryView[];
  /** Returns 'none', 'active', or 'completed'. */
  questStatus(questId: string): QuestStatus;
  /** Returns true if the quest has been started or completed. */
  hasQuest(questId: string): boolean;
  /** Grants the quest if not yet started. Returns true when newly granted. */
  giveQuest(questId: string, title?: string): boolean;
  /** Marks an active or unseen quest as 'completed'. */
  completeQuest(questId: string): void;
  /** Teleports the player within the current area to (x, z) with optional heading in degrees. */
  teleport(x: number, z: number, yawDeg?: number): void;
  /** Transfers the player into another area at (x, z) with optional heading in degrees. */
  moveToArea(areaId: AreaId, x: number, z: number, yawDeg?: number): void;
  /** Transfers the player to an entity with TeleportTarget in areaId matching targetName. */
  teleportToArea(areaId: AreaId, targetName: string): void;
}

interface TriggerContext {
  /** Whether triggered by 'click' or 'walk_into'. */
  readonly way: TriggerWay;
  /** Area ID where the trigger resides. */
  readonly areaId: AreaId;
  /** Known entity template IDs in the project. */
  readonly templateIdRef?: TemplateId;
  /** The triggered entity instance. */
  readonly entity: ScriptEntityApi;
  /** The interacting or overlapping player entity. */
  readonly player: ScriptPlayerApi;
  /** Shorthand for ctx.player.hasItem(itemId, minCount). */
  hasItem(itemId: ItemId, minCount?: number): boolean;
  /** Transfers the player to an entity with TeleportTarget in areaId matching targetName. */
  teleportToArea(areaId: AreaId, targetName: string): void;
  /** Displays a holographic popup dialog on the triggering player's HUD. */
  showPopup(title: string, body: string, tone?: PopupTone): void;
  /** Writes a diagnostic message to the server log. */
  log(message: string): void;
}
`;
}
