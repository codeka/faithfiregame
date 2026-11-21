import { Component, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { CharacterEditorStateService } from '../../character-editor-state.service';
import {
  ComponentOverride,
  EditorEngineService,
  EntityOverrides,
  MovementConfig,
  PlacedEntity,
  PropertyValue,
  TriggerActivation,
  TriggerConfig,
  VisualConfig,
} from '../../editor-engine.service';
import { EditorStateService } from '../../editor-state.service';
import {
  defaultMovementConfig,
  defaultPropertyValue,
  defaultTriggerConfig,
  defaultVisualConfig,
  EntityEditorStateService,
  isOverridesEmpty,
  PropertyKind,
  propertyTypeOf,
  resolvePlacedEntity,
} from '../../entity-editor-state.service';

export const ENTITY_SUBMODES = [
  { id: 'place', label: 'Place', shortcut: '1' },
  { id: 'move', label: 'Move', shortcut: '2' },
  { id: 'free_move', label: 'Free Move', shortcut: '3' },
  { id: 'rotate', label: 'Rotate', shortcut: '4' },
] as const;

@Component({
  selector: 'app-entity-tool',
  standalone: true,
  imports: [FormsModule],
  templateUrl: './entity-tool.component.html',
  styleUrls: ['../../editor-controls.css', './entity-tool.component.css'],
})
export class EntityToolComponent {
  readonly engine = inject(EditorEngineService);
  readonly worldState = inject(EditorStateService);
  readonly entityState = inject(EntityEditorStateService);
  readonly charState = inject(CharacterEditorStateService);

  readonly submodes = ENTITY_SUBMODES;
  readonly newPropKey = signal('');
  readonly newPropType = signal<PropertyKind>('String');

  readonly activeSubmode = computed(() => this.engine.status()?.entitySubmode ?? 'place');
  readonly activeTemplateId = computed(() => this.engine.status()?.activeEntityTemplateId ?? '');
  readonly previewYawDeg = computed(() => {
    const deg = this.engine.status()?.entityPreviewYawDeg ?? 0;
    return ((Math.round(deg) % 360) + 360) % 360;
  });

  readonly selectedEntity = computed(() => this.engine.status()?.selectedEntity ?? null);
  readonly linkedTemplate = computed(() => {
    const sel = this.selectedEntity();
    if (!sel) return null;
    return this.entityState.templates().find((t) => t.id === sel.template_id) ?? null;
  });
  readonly resolved = computed(() => {
    const sel = this.selectedEntity();
    if (!sel) return null;
    return resolvePlacedEntity(sel, this.linkedTemplate());
  });
  readonly hasAnyOverrides = computed(() => {
    const sel = this.selectedEntity();
    return sel ? !isOverridesEmpty(sel.overrides) : false;
  });
  readonly selectedYawDeg = computed(() => {
    const sel = this.selectedEntity();
    if (!sel) return 0;
    const deg = (sel.rotation_y * 180) / Math.PI;
    return ((Math.round(deg) % 360) + 360) % 360;
  });

  readonly propertyEntries = computed(() => {
    const sel = this.selectedEntity();
    const res = this.resolved();
    const tpl = this.linkedTemplate();
    if (!sel || !res) return [];
    return Object.entries(res.properties)
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([key, value]) => ({
        key,
        value,
        kind: propertyTypeOf(value),
        overridden: Object.prototype.hasOwnProperty.call(sel.overrides.properties ?? {}, key),
        inTemplate: Boolean(tpl && Object.prototype.hasOwnProperty.call(tpl.properties ?? {}, key)),
      }));
  });

  onSearchInput(ev: Event): void {
    this.entityState.templateSearch.set((ev.target as HTMLInputElement).value);
  }

  clearSearch(): void {
    this.entityState.templateSearch.set('');
  }

  selectPlacementTemplate(id: string): void {
    this.engine.setPlacementEntityTemplate(id);
  }

  private pushUpdated(next: PlacedEntity): void {
    this.engine.updateSelectedPlacedEntity(next);
  }

  private cloneOverrides(sel: PlacedEntity): EntityOverrides {
    return {
      name: sel.overrides.name,
      properties: structuredClone(sel.overrides.properties ?? {}),
      removed_properties: [...(sel.overrides.removed_properties ?? [])],
      visual: structuredClone(sel.overrides.visual),
      movement: structuredClone(sel.overrides.movement),
      trigger: structuredClone(sel.overrides.trigger),
    };
  }

  onYawInput(ev: Event): void {
    const sel = this.selectedEntity();
    if (!sel) return;
    const deg = (ev.target as HTMLInputElement).valueAsNumber;
    if (!Number.isFinite(deg)) return;
    const rad = (((deg % 360) + 360) % 360) * (Math.PI / 180);
    this.pushUpdated({ ...sel, rotation_y: rad });
  }

  resetAllOverrides(): void {
    const sel = this.selectedEntity();
    if (!sel) return;
    this.pushUpdated({
      ...sel,
      overrides: {
        name: null,
        properties: {},
        removed_properties: [],
        visual: 'inherit',
        movement: 'inherit',
        trigger: 'inherit',
      },
    });
  }

  setNameOverride(name: string): void {
    const sel = this.selectedEntity();
    if (!sel) return;
    const ov = this.cloneOverrides(sel);
    ov.name = name;
    this.pushUpdated({ ...sel, overrides: ov });
  }

  resetNameOverride(): void {
    const sel = this.selectedEntity();
    if (!sel) return;
    const ov = this.cloneOverrides(sel);
    ov.name = null;
    this.pushUpdated({ ...sel, overrides: ov });
  }

  setPropertyOverride(key: string, val: PropertyValue): void {
    const sel = this.selectedEntity();
    const trimmed = key.trim();
    if (!sel || !trimmed) return;
    const ov = this.cloneOverrides(sel);
    ov.properties[trimmed] = val;
    ov.removed_properties = ov.removed_properties.filter((k) => k !== trimmed);
    this.pushUpdated({ ...sel, overrides: ov });
  }

  resetPropertyOverride(key: string): void {
    const sel = this.selectedEntity();
    if (!sel) return;
    const ov = this.cloneOverrides(sel);
    delete ov.properties[key];
    ov.removed_properties = ov.removed_properties.filter((k) => k !== key);
    this.pushUpdated({ ...sel, overrides: ov });
  }

  removePropertyOverride(key: string, inTemplate: boolean): void {
    const sel = this.selectedEntity();
    if (!sel) return;
    const ov = this.cloneOverrides(sel);
    delete ov.properties[key];
    if (inTemplate) {
      if (!ov.removed_properties.includes(key)) {
        ov.removed_properties = [...ov.removed_properties, key].sort();
      }
    } else {
      ov.removed_properties = ov.removed_properties.filter((k) => k !== key);
    }
    this.pushUpdated({ ...sel, overrides: ov });
  }

  restoreRemovedProperty(key: string): void {
    this.resetPropertyOverride(key);
  }

  addProperty(): void {
    const key = this.newPropKey().trim();
    if (!key) return;
    this.setPropertyOverride(key, defaultPropertyValue(this.newPropType()));
    this.newPropKey.set('');
  }

  propBool(val: PropertyValue): boolean {
    return 'Bool' in val ? val.Bool : false;
  }

  propNumber(val: PropertyValue): number {
    if ('Int' in val) return val.Int;
    if ('Float' in val) return val.Float;
    return 0;
  }

  propString(val: PropertyValue): string {
    return 'String' in val ? val.String : '';
  }

  onBoolPropChange(key: string, ev: Event): void {
    this.setPropertyOverride(key, { Bool: (ev.target as HTMLInputElement).checked });
  }

  onIntPropChange(key: string, val: number): void {
    if (!Number.isFinite(val)) return;
    this.setPropertyOverride(key, { Int: Math.round(val) });
  }

  onFloatPropChange(key: string, val: number): void {
    if (!Number.isFinite(val)) return;
    this.setPropertyOverride(key, { Float: val });
  }

  onStringPropChange(key: string, val: string): void {
    this.setPropertyOverride(key, { String: val });
  }

  overrideStatusLabel<T>(ov: ComponentOverride<T>, templateHas: boolean): string {
    if (ov === 'remove') return 'Removed';
    if (ov === 'inherit') return templateHas ? 'Inherited' : 'None';
    return 'Overridden';
  }

  setVisualOverride(ovVal: ComponentOverride<VisualConfig>): void {
    const sel = this.selectedEntity();
    if (!sel) return;
    const ov = this.cloneOverrides(sel);
    ov.visual = ovVal;
    this.pushUpdated({ ...sel, overrides: ov });
  }

  addOrRestoreVisual(): void {
    const sel = this.selectedEntity();
    if (!sel) return;
    if (sel.overrides.visual === 'remove' && this.linkedTemplate()?.visual) {
      this.setVisualOverride('inherit');
      return;
    }
    const firstPreset = this.charState.presets()[0]?.id ?? 'cryo_prisoner';
    const firstAsset = this.worldState.catalog()[0]?.id ?? 'pine_tree';
    const kind = this.charState.presets().length > 0 ? 'character' : 'prop';
    this.setVisualOverride({ set: defaultVisualConfig(kind, firstPreset, firstAsset) });
  }

  removeVisual(): void {
    const tplHas = Boolean(this.linkedTemplate()?.visual);
    this.setVisualOverride(tplHas ? 'remove' : 'inherit');
  }

  isCharacterVisual(vis: VisualConfig): vis is { character: { preset_id: string } } {
    return 'character' in vis;
  }

  isPropVisual(
    vis: VisualConfig,
  ): vis is { prop: { asset_id: string; scale: [number, number, number]; cast_shadow: boolean } } {
    return 'prop' in vis;
  }

  setVisualKind(kind: 'character' | 'prop'): void {
    const firstPreset = this.charState.presets()[0]?.id ?? 'cryo_prisoner';
    const firstAsset = this.worldState.catalog()[0]?.id ?? 'pine_tree';
    this.setVisualOverride({ set: defaultVisualConfig(kind, firstPreset, firstAsset) });
  }

  setCharacterPreset(presetId: string): void {
    this.setVisualOverride({ set: { character: { preset_id: presetId } } });
  }

  setPropAsset(assetId: string): void {
    const cur = this.resolved()?.visual;
    if (!cur || !this.isPropVisual(cur)) return;
    this.setVisualOverride({ set: { prop: { ...cur.prop, asset_id: assetId } } });
  }

  setPropScale(scale: number): void {
    const cur = this.resolved()?.visual;
    if (!cur || !this.isPropVisual(cur) || !Number.isFinite(scale)) return;
    const s = Math.max(0.05, Math.min(20, scale));
    this.setVisualOverride({ set: { prop: { ...cur.prop, scale: [s, s, s] } } });
  }

  onPropCastShadowChange(ev: Event): void {
    const cur = this.resolved()?.visual;
    if (!cur || !this.isPropVisual(cur)) return;
    this.setVisualOverride({
      set: { prop: { ...cur.prop, cast_shadow: (ev.target as HTMLInputElement).checked } },
    });
  }

  setMovementOverride(ovVal: ComponentOverride<MovementConfig>): void {
    const sel = this.selectedEntity();
    if (!sel) return;
    const ov = this.cloneOverrides(sel);
    ov.movement = ovVal;
    this.pushUpdated({ ...sel, overrides: ov });
  }

  addOrRestoreMovement(): void {
    const sel = this.selectedEntity();
    if (!sel) return;
    if (sel.overrides.movement === 'remove' && this.linkedTemplate()?.movement) {
      this.setMovementOverride('inherit');
      return;
    }
    this.setMovementOverride({ set: defaultMovementConfig() });
  }

  removeMovement(): void {
    const tplHas = Boolean(this.linkedTemplate()?.movement);
    this.setMovementOverride(tplHas ? 'remove' : 'inherit');
  }

  setMovementSpeed(speed: number): void {
    if (!Number.isFinite(speed)) return;
    this.setMovementOverride({ set: { speed: Math.max(0.1, speed) } });
  }

  setTriggerOverride(ovVal: ComponentOverride<TriggerConfig>): void {
    const sel = this.selectedEntity();
    if (!sel) return;
    const ov = this.cloneOverrides(sel);
    ov.trigger = ovVal;
    this.pushUpdated({ ...sel, overrides: ov });
  }

  addOrRestoreTrigger(): void {
    const sel = this.selectedEntity();
    if (!sel) return;
    if (sel.overrides.trigger === 'remove' && this.linkedTemplate()?.trigger) {
      this.setTriggerOverride('inherit');
      return;
    }
    this.setTriggerOverride({ set: defaultTriggerConfig() });
  }

  removeTrigger(): void {
    const tplHas = Boolean(this.linkedTemplate()?.trigger);
    this.setTriggerOverride(tplHas ? 'remove' : 'inherit');
  }

  private patchTrigger(patch: Partial<TriggerConfig>): void {
    const cur = this.resolved()?.trigger;
    if (!cur) return;
    this.setTriggerOverride({ set: { ...cur, ...patch } });
  }

  setTriggerActivation(activation: TriggerActivation): void {
    this.patchTrigger({ activation });
  }

  setTriggerSize(axis: 0 | 1 | 2, val: number): void {
    const cur = this.resolved()?.trigger;
    if (!cur || !Number.isFinite(val)) return;
    const size: [number, number, number] = [...cur.size];
    size[axis] = Math.max(0.1, val);
    this.patchTrigger({ size });
  }

  setTriggerOffset(axis: 0 | 1 | 2, val: number): void {
    const cur = this.resolved()?.trigger;
    if (!cur || !Number.isFinite(val)) return;
    const offset: [number, number, number] = [...cur.offset];
    offset[axis] = val;
    this.patchTrigger({ offset });
  }

  setTriggerYaw(yawDeg: number): void {
    if (!Number.isFinite(yawDeg)) return;
    this.patchTrigger({ yaw_deg: yawDeg });
  }

  setTriggerRange(range: number): void {
    if (!Number.isFinite(range)) return;
    this.patchTrigger({ interaction_range: Math.max(0.1, range) });
  }
}
