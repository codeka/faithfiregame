import { computed, inject, Injectable, signal } from '@angular/core';
import {
  ComponentOverride,
  EditorEngineService,
  EntityOverrides,
  EntityTemplate,
  InventoryConfig,
  ItemCatalog,
  ItemDef,
  LootableConfig,
  MovementConfig,
  PlacedEntity,
  PropertyValue,
  ScriptConfig,
  TeleportTargetConfig,
  TriggerConfig,
  VisualConfig,
} from './editor-engine.service';

function sanitizeSlug(raw: string, fallback = 'entity_template'): string {
  const slug = raw
    .trim()
    .replace(/[^a-zA-Z0-9-]+/g, '_')
    .replace(/^[_-]+|[_-]+$/g, '')
    .toLowerCase()
    .slice(0, 64);
  return slug || fallback;
}

function prettifySlug(slug: string): string {
  return slug
    .split(/[_-]+/)
    .filter(Boolean)
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(' ');
}

export const POSE_CLIP_OPTIONS: readonly string[] = ['DeadSlumped', 'DeadSupine', 'DeadProne'];

export function defaultVisualConfig(
  kind: 'character' | 'prop',
  defaultPresetId: string,
  defaultAssetId: string,
): VisualConfig {
  if (kind === 'character') {
    return { character: { preset_id: defaultPresetId || 'cryo_prisoner', pose_clip: null } };
  }
  return {
    prop: {
      asset_id: defaultAssetId || 'pine_tree',
      scale: [1, 1, 1],
      cast_shadow: true,
    },
  };
}

export function defaultMovementConfig(): MovementConfig {
  return { speed: 3.5 };
}

export function defaultTriggerConfig(): TriggerConfig {
  return {
    size: [1.6, 2.0, 1.6],
    offset: [0, 0, 0],
    yaw_deg: 0,
    activation: 'click',
    interaction_range: 2.5,
  };
}

export function defaultInventoryConfig(): InventoryConfig {
  return { items: [] };
}

export function defaultLootableConfig(): LootableConfig {
  return { title: null, description: null };
}

export function defaultTeleportTargetConfig(): TeleportTargetConfig {
  return {};
}

export type PropertyKind = 'Bool' | 'Int' | 'Float' | 'String';

export function propertyTypeOf(val: PropertyValue): PropertyKind {
  if ('Bool' in val) return 'Bool';
  if ('Int' in val) return 'Int';
  if ('Float' in val) return 'Float';
  return 'String';
}

export function defaultPropertyValue(kind: PropertyKind): PropertyValue {
  switch (kind) {
    case 'Bool':
      return { Bool: true };
    case 'Int':
      return { Int: 0 };
    case 'Float':
      return { Float: 1.0 };
    case 'String':
      return { String: '' };
  }
}

export function isOverridesEmpty(ov: EntityOverrides): boolean {
  return (
    ov.name === null &&
    Object.keys(ov.properties ?? {}).length === 0 &&
    (ov.removed_properties ?? []).length === 0 &&
    ov.visual === 'inherit' &&
    ov.movement === 'inherit' &&
    ov.trigger === 'inherit' &&
    ov.script === 'inherit' &&
    (ov.inventory ?? 'inherit') === 'inherit' &&
    (ov.lootable ?? 'inherit') === 'inherit' &&
    (ov.teleport_target ?? 'inherit') === 'inherit'
  );
}

export function resolveComponentOverride<T>(
  ov: ComponentOverride<T>,
  base: T | null | undefined,
): T | null {
  if (ov === 'remove') return null;
  if (ov === 'inherit') return base ? structuredClone(base) : null;
  return structuredClone(ov.set);
}

export interface ResolvedPlacedEntityView {
  name: string;
  properties: Record<string, PropertyValue>;
  visual: VisualConfig | null;
  movement: MovementConfig | null;
  trigger: TriggerConfig | null;
  script: ScriptConfig | null;
  inventory: InventoryConfig | null;
  lootable: LootableConfig | null;
  teleport_target: TeleportTargetConfig | null;
}

export function resolvePlacedEntity(
  placed: PlacedEntity,
  template: EntityTemplate | null | undefined,
): ResolvedPlacedEntityView {
  const baseName = template?.name ?? prettifySlug(placed.template_id);
  const name = placed.overrides.name ?? baseName;

  const properties: Record<string, PropertyValue> = structuredClone(template?.properties ?? {});
  for (const key of placed.overrides.removed_properties ?? []) {
    delete properties[key];
  }
  for (const [k, v] of Object.entries(placed.overrides.properties ?? {})) {
    properties[k] = structuredClone(v);
  }

  return {
    name,
    properties,
    visual: resolveComponentOverride(placed.overrides.visual, template?.visual),
    movement: resolveComponentOverride(placed.overrides.movement, template?.movement),
    trigger: resolveComponentOverride(placed.overrides.trigger, template?.trigger),
    script: resolveComponentOverride(placed.overrides.script, template?.script),
    inventory: resolveComponentOverride(placed.overrides.inventory ?? 'inherit', template?.inventory),
    lootable: resolveComponentOverride(placed.overrides.lootable ?? 'inherit', template?.lootable),
    teleport_target: resolveComponentOverride(
      placed.overrides.teleport_target ?? 'inherit',
      template?.teleport_target,
    ),
  };
}

export type ScriptModalTarget = { kind: 'template' } | { kind: 'instance' } | null;

@Injectable()
export class EntityEditorStateService {
  private readonly engine = inject(EditorEngineService);

  readonly templates = signal<EntityTemplate[]>([]);
  readonly items = signal<ItemDef[]>([]);
  readonly activeTemplate = signal<EntityTemplate | null>(null);
  readonly dirty = signal<boolean>(false);
  readonly saveState = signal<string>('Saved');
  readonly templateSearch = signal<string>('');
  readonly scriptModalTarget = signal<ScriptModalTarget>(null);

  readonly filteredTemplates = computed<readonly EntityTemplate[]>(() => {
    const tokens = this.templateSearch().trim().toLowerCase().split(/\s+/).filter(Boolean);
    if (tokens.length === 0) {
      return this.templates();
    }
    return this.templates().filter((t) => {
      const hay = `${t.name} ${t.id}`.toLowerCase();
      return tokens.every((tok) => hay.includes(tok));
    });
  });

  async initData(): Promise<void> {
    await Promise.all([this.loadTemplates(), this.loadItemCatalog()]);
    const list = this.templates();
    if (list.length > 0 && !this.activeTemplate()) {
      this.applyActiveTemplate(list[0], false);
      this.engine.setPlacementEntityTemplate(list[0].id);
    }
  }

  async loadItemCatalog(): Promise<void> {
    try {
      const resp = await fetch('/api/editor/items/catalog', { cache: 'no-cache' });
      if (resp.ok) {
        const cat = (await resp.json()) as ItemCatalog;
        this.items.set(cat.items ?? []);
      }
    } catch (e) {
      console.error('Failed to load item catalog:', e);
    }
  }

  async loadTemplates(): Promise<void> {
    try {
      const resp = await fetch('/api/editor/entities/templates', { cache: 'no-cache' });
      if (resp.ok) {
        const raw = (await resp.json()) as EntityTemplate[];
        const list = raw.map((t) => this.normalizeTemplate(t));
        this.templates.set(list);
        this.engine.setEntityTemplates(list);
      }
    } catch (e) {
      console.error('Failed to load entity templates:', e);
    }
  }

  private normalizeTemplate(tpl: EntityTemplate): EntityTemplate {
    return {
      ...tpl,
      properties: { ...(tpl.properties ?? {}) },
      visual: tpl.visual ? structuredClone(tpl.visual) : null,
      movement: tpl.movement ? structuredClone(tpl.movement) : null,
      trigger: tpl.trigger ? structuredClone(tpl.trigger) : null,
      script: tpl.script ? structuredClone(tpl.script) : null,
      inventory: tpl.inventory
        ? { items: structuredClone(tpl.inventory.items ?? []) }
        : null,
      lootable: tpl.lootable ? structuredClone(tpl.lootable) : null,
      teleport_target: tpl.teleport_target ? structuredClone(tpl.teleport_target) : null,
    };
  }

  private applyActiveTemplate(tpl: EntityTemplate | null, dirty: boolean): void {
    const next = tpl ? this.normalizeTemplate(tpl) : null;
    this.activeTemplate.set(next);
    this.dirty.set(dirty);
    this.saveState.set(dirty ? 'Unsaved' : 'Saved');
    this.engine.setActiveEntityTemplate(next);
  }

  selectTemplate(templateId: string): void {
    const found = this.templates().find((t) => t.id === templateId) ?? null;
    if (found) {
      this.applyActiveTemplate(found, false);
      this.engine.setPlacementEntityTemplate(found.id);
    }
  }

  private uniqueTemplateId(baseSlug: string): string {
    const existing = new Set(this.templates().map((t) => t.id));
    if (!existing.has(baseSlug)) return baseSlug;
    let n = 2;
    while (existing.has(`${baseSlug}_${n}`)) {
      n += 1;
    }
    return `${baseSlug}_${n}`;
  }

  createNewTemplate(name = 'New Entity'): void {
    const id = this.uniqueTemplateId(sanitizeSlug(name, 'new_entity'));
    const created: EntityTemplate = {
      id,
      name,
      properties: {},
      visual: null,
      movement: null,
      trigger: null,
      script: null,
      inventory: null,
      lootable: null,
      teleport_target: null,
    };
    const nextList = [...this.templates(), created];
    this.templates.set(nextList);
    this.engine.setEntityTemplates(nextList);
    this.engine.setPlacementEntityTemplate(created.id);
    this.applyActiveTemplate(created, true);
  }

  duplicateTemplate(): void {
    const cur = this.activeTemplate();
    if (!cur) return;
    const id = this.uniqueTemplateId(`${cur.id}_copy`);
    const dup: EntityTemplate = {
      ...this.normalizeTemplate(cur),
      id,
      name: `${cur.name} Copy`,
    };
    const nextList = [...this.templates(), dup];
    this.templates.set(nextList);
    this.engine.setEntityTemplates(nextList);
    this.engine.setPlacementEntityTemplate(dup.id);
    this.applyActiveTemplate(dup, true);
  }

  async deleteActiveTemplate(): Promise<void> {
    const cur = this.activeTemplate();
    if (!cur) return;
    try {
      const resp = await fetch(`/api/editor/entities/templates/${encodeURIComponent(cur.id)}`, {
        method: 'DELETE',
      });
      if (!resp.ok && resp.status !== 404) {
        throw new Error(`Delete failed (HTTP ${resp.status})`);
      }
      const nextList = this.templates().filter((t) => t.id !== cur.id);
      this.templates.set(nextList);
      this.engine.setEntityTemplates(nextList);
      if (nextList.length > 0) {
        this.applyActiveTemplate(nextList[0], false);
        this.engine.setPlacementEntityTemplate(nextList[0].id);
      } else {
        this.applyActiveTemplate(null, false);
        this.engine.setPlacementEntityTemplate('');
      }
    } catch (e) {
      console.error('Failed deleting entity template:', e);
      this.saveState.set('Save failed');
    }
  }

  private mutateActive(mutator: (tpl: EntityTemplate) => EntityTemplate): void {
    const cur = this.activeTemplate();
    if (!cur) return;
    const updated = this.normalizeTemplate(mutator(this.normalizeTemplate(cur)));
    this.activeTemplate.set(updated);
    const nextList = this.templates().map((t) => (t.id === updated.id ? updated : t));
    this.templates.set(nextList);
    this.dirty.set(true);
    this.saveState.set('Unsaved');
    this.engine.setEntityTemplates(nextList);
    this.engine.setActiveEntityTemplate(updated);
  }

  setName(name: string): void {
    this.mutateActive((t) => ({ ...t, name }));
  }

  setProperty(key: string, value: PropertyValue): void {
    const trimmed = key.trim();
    if (!trimmed) return;
    this.mutateActive((t) => ({
      ...t,
      properties: { ...t.properties, [trimmed]: value },
    }));
  }

  removeProperty(key: string): void {
    this.mutateActive((t) => {
      const next = { ...t.properties };
      delete next[key];
      return { ...t, properties: next };
    });
  }

  setVisual(visual: VisualConfig | null): void {
    this.mutateActive((t) => ({ ...t, visual }));
  }

  setMovement(movement: MovementConfig | null): void {
    this.mutateActive((t) => ({ ...t, movement }));
  }

  setTrigger(trigger: TriggerConfig | null): void {
    this.mutateActive((t) => ({ ...t, trigger }));
  }

  setScript(script: ScriptConfig | null): void {
    this.mutateActive((t) => ({ ...t, script }));
  }

  setInventory(inventory: InventoryConfig | null): void {
    this.mutateActive((t) => ({ ...t, inventory }));
  }

  setLootable(lootable: LootableConfig | null): void {
    this.mutateActive((t) => ({ ...t, lootable }));
  }

  setTeleportTarget(teleport_target: TeleportTargetConfig | null): void {
    this.mutateActive((t) => ({ ...t, teleport_target }));
  }

  openTemplateScriptModal(): void {
    this.scriptModalTarget.set({ kind: 'template' });
  }

  openInstanceScriptModal(): void {
    this.scriptModalTarget.set({ kind: 'instance' });
  }

  closeScriptModal(): void {
    this.scriptModalTarget.set(null);
  }

  async saveActiveTemplate(): Promise<boolean> {
    const cur = this.activeTemplate();
    if (!cur) return false;
    this.saveState.set('Saving...');
    try {
      const resp = await fetch(`/api/editor/entities/templates/${encodeURIComponent(cur.id)}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(cur),
      });
      if (!resp.ok) {
        this.saveState.set('Save failed');
        return false;
      }
      const saved = this.normalizeTemplate((await resp.json()) as EntityTemplate);
      const nextList = this.templates().some((t) => t.id === saved.id)
        ? this.templates().map((t) => (t.id === saved.id ? saved : t))
        : [...this.templates(), saved].sort((a, b) => a.id.localeCompare(b.id));
      this.templates.set(nextList);
      this.activeTemplate.set(saved);
      this.dirty.set(false);
      this.saveState.set('Saved');
      this.engine.setEntityTemplates(nextList);
      this.engine.setActiveEntityTemplate(saved);
      return true;
    } catch (e) {
      console.error('Failed saving entity template:', e);
      this.saveState.set('Save failed');
      return false;
    }
  }
}
