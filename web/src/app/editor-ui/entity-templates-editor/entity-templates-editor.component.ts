import { Component, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { CharacterEditorStateService } from '../character-editor-state.service';
import {
  PropertyValue,
  TriggerActivation,
  TriggerConfig,
  VisualConfig,
} from '../editor-engine.service';
import { EditorStateService } from '../editor-state.service';
import {
  defaultMovementConfig,
  defaultPropertyValue,
  defaultTriggerConfig,
  defaultVisualConfig,
  EntityEditorStateService,
  PropertyKind,
  propertyTypeOf,
} from '../entity-editor-state.service';
import { defaultScriptConfig } from '../script-compiler';

@Component({
  selector: 'app-entity-templates-editor',
  standalone: true,
  imports: [FormsModule],
  templateUrl: './entity-templates-editor.component.html',
  styleUrls: ['../editor-controls.css', './entity-templates-editor.component.css'],
})
export class EntityTemplatesEditorComponent {
  readonly entityState = inject(EntityEditorStateService);
  readonly charState = inject(CharacterEditorStateService);
  readonly worldState = inject(EditorStateService);

  readonly newPropKey = signal('');
  readonly newPropType = signal<PropertyKind>('String');

  readonly template = computed(() => this.entityState.activeTemplate());
  readonly propertyEntries = computed(() => {
    const tpl = this.template();
    if (!tpl) return [];
    return Object.entries(tpl.properties)
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([key, value]) => ({
        key,
        value,
        kind: propertyTypeOf(value),
      }));
  });

  addProperty(): void {
    const key = this.newPropKey().trim();
    if (!key) return;
    this.entityState.setProperty(key, defaultPropertyValue(this.newPropType()));
    this.newPropKey.set('');
  }

  onBoolPropChange(key: string, ev: Event): void {
    this.entityState.setProperty(key, { Bool: (ev.target as HTMLInputElement).checked });
  }

  onIntPropChange(key: string, val: number): void {
    if (!Number.isFinite(val)) return;
    this.entityState.setProperty(key, { Int: Math.round(val) });
  }

  onFloatPropChange(key: string, val: number): void {
    if (!Number.isFinite(val)) return;
    this.entityState.setProperty(key, { Float: val });
  }

  onStringPropChange(key: string, val: string): void {
    this.entityState.setProperty(key, { String: val });
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

  addVisualComponent(): void {
    const firstPreset = this.charState.presets()[0]?.id ?? 'cryo_prisoner';
    const firstAsset = this.worldState.catalog()[0]?.id ?? 'pine_tree';
    const kind = this.charState.presets().length > 0 ? 'character' : 'prop';
    this.entityState.setVisual(defaultVisualConfig(kind, firstPreset, firstAsset));
  }

  setVisualKind(kind: 'character' | 'prop'): void {
    const firstPreset = this.charState.presets()[0]?.id ?? 'cryo_prisoner';
    const firstAsset = this.worldState.catalog()[0]?.id ?? 'pine_tree';
    this.entityState.setVisual(defaultVisualConfig(kind, firstPreset, firstAsset));
  }

  isCharacterVisual(vis: VisualConfig): vis is { character: { preset_id: string } } {
    return 'character' in vis;
  }

  isPropVisual(
    vis: VisualConfig,
  ): vis is { prop: { asset_id: string; scale: [number, number, number]; cast_shadow: boolean } } {
    return 'prop' in vis;
  }

  setCharacterPreset(presetId: string): void {
    this.entityState.setVisual({ character: { preset_id: presetId } });
  }

  setPropAsset(assetId: string): void {
    const cur = this.template()?.visual;
    if (!cur || !this.isPropVisual(cur)) return;
    this.entityState.setVisual({ prop: { ...cur.prop, asset_id: assetId } });
  }

  setPropScale(scale: number): void {
    const cur = this.template()?.visual;
    if (!cur || !this.isPropVisual(cur) || !Number.isFinite(scale)) return;
    const s = Math.max(0.05, Math.min(20, scale));
    this.entityState.setVisual({ prop: { ...cur.prop, scale: [s, s, s] } });
  }

  onPropCastShadowChange(ev: Event): void {
    const cur = this.template()?.visual;
    if (!cur || !this.isPropVisual(cur)) return;
    this.entityState.setVisual({
      prop: { ...cur.prop, cast_shadow: (ev.target as HTMLInputElement).checked },
    });
  }

  addMovementComponent(): void {
    this.entityState.setMovement(defaultMovementConfig());
  }

  setMovementSpeed(speed: number): void {
    if (!Number.isFinite(speed)) return;
    this.entityState.setMovement({ speed: Math.max(0.1, speed) });
  }

  addTriggerComponent(): void {
    this.entityState.setTrigger(defaultTriggerConfig());
  }

  private patchTrigger(patch: Partial<TriggerConfig>): void {
    const cur = this.template()?.trigger;
    if (!cur) return;
    this.entityState.setTrigger({ ...cur, ...patch });
  }

  setTriggerActivation(activation: TriggerActivation): void {
    this.patchTrigger({ activation });
  }

  setTriggerSize(axis: 0 | 1 | 2, val: number): void {
    const cur = this.template()?.trigger;
    if (!cur || !Number.isFinite(val)) return;
    const size: [number, number, number] = [...cur.size];
    size[axis] = Math.max(0.1, val);
    this.patchTrigger({ size });
  }

  setTriggerOffset(axis: 0 | 1 | 2, val: number): void {
    const cur = this.template()?.trigger;
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

  addScriptComponent(): void {
    this.entityState.setScript(defaultScriptConfig());
  }

  scriptPreview(source: string): string {
    return source.trim().split(/\r?\n/).slice(0, 4).join('\n');
  }
}
