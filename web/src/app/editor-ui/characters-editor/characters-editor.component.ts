import { Component, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import {
  CharacterEditorStateService,
  ImportCharacterGlbTarget,
  rgbToHex,
} from '../character-editor-state.service';
import { CharacterPalette, EditorEngineService } from '../editor-engine.service';

const CANONICAL_BONES = [
  'root',
  'pelvis',
  'spine_01',
  'spine_02',
  'neck',
  'head',
  'clavicle_l',
  'upperarm_l',
  'lowerarm_l',
  'hand_l',
  'clavicle_r',
  'upperarm_r',
  'lowerarm_r',
  'hand_r',
  'thigh_l',
  'calf_l',
  'foot_l',
  'thigh_r',
  'calf_r',
  'foot_r',
];

const PALETTE_CHANNELS: readonly {
  key: keyof CharacterPalette;
  label: string;
  defaultHex: string;
}[] = [
  { key: 'skin_color', label: 'Skin', defaultHex: '#c7997a' },
  { key: 'hair_color', label: 'Hair', defaultHex: '#33261c' },
  { key: 'eye_color', label: 'Eyes', defaultHex: '#1f2429' },
  { key: 'primary_color', label: 'Primary', defaultHex: '#386b8f' },
  { key: 'secondary_color', label: 'Secondary', defaultHex: '#474f40' },
  { key: 'accent_color', label: 'Accent', defaultHex: '#b8382e' },
];

const SPEED_OPTIONS = [0.25, 0.5, 1, 1.5] as const;

@Component({
  selector: 'app-characters-editor',
  standalone: true,
  imports: [FormsModule],
  templateUrl: './characters-editor.component.html',
  styleUrls: ['../editor-controls.css', './characters-editor.component.css'],
})
export class CharactersEditorComponent {
  readonly engine = inject(EditorEngineService);
  readonly charState = inject(CharacterEditorStateService);

  readonly paletteChannels = PALETTE_CHANNELS;
  readonly speedOptions = SPEED_OPTIONS;

  readonly dragOver = signal(false);
  readonly importKind = signal<'skinned' | 'socket' | 'animation' | 'skeleton'>('skinned');
  readonly importSlot = signal<string>('torso');
  readonly importSocket = signal<string>('socket_hand_r');
  readonly importCategory = signal<string>('Custom');
  readonly importSkeletonName = signal<string>('');

  readonly newSlotName = signal<string>('');
  readonly newSocketName = signal<string>('');
  readonly newSocketBone = signal<string>('head');

  readonly studioStatus = computed(() => this.engine.status()?.characterStudio ?? null);
  readonly boneOptions = computed<readonly string[]>(() => {
    const bones = this.studioStatus()?.boneNames ?? [];
    return bones.length > 0 ? bones : CANONICAL_BONES;
  });

  paletteHex(key: keyof CharacterPalette, defaultHex: string): string {
    return rgbToHex(this.charState.activeCharacter().palette[key], defaultHex);
  }

  hasPaletteOverride(key: keyof CharacterPalette): boolean {
    return Boolean(this.charState.activeCharacter().palette[key]);
  }

  onPaletteInput(key: keyof CharacterPalette, ev: Event): void {
    this.charState.setPaletteColor(key, (ev.target as HTMLInputElement).value);
  }

  onShowBaseMeshChange(ev: Event): void {
    this.charState.setShowBaseMesh((ev.target as HTMLInputElement).checked);
  }

  onScrubInput(ev: Event): void {
    this.engine.setCharacterAnimTime((ev.target as HTMLInputElement).valueAsNumber);
  }

  addCustomSlot(): void {
    const label = this.newSlotName().trim();
    if (!label) return;
    this.charState.addSkinnedSlot(label, label);
    this.newSlotName.set('');
  }

  addCustomSocket(): void {
    const label = this.newSocketName().trim();
    if (!label) return;
    this.charState.addSocket(label, label, this.newSocketBone());
    this.newSocketName.set('');
  }

  updateSocketOffset(axis: 0 | 1 | 2, val: number): void {
    const sock = this.charState.selectedSocket();
    if (!sock || !Number.isFinite(val)) return;
    const next: [number, number, number] = [...sock.offset];
    next[axis] = val;
    this.charState.updateSocket(sock.id, { offset: next });
  }

  updateSocketRotation(axis: 0 | 1 | 2, val: number): void {
    const sock = this.charState.selectedSocket();
    if (!sock || !Number.isFinite(val)) return;
    const next: [number, number, number] = [...sock.rotation_deg];
    next[axis] = val;
    this.charState.updateSocket(sock.id, { rotation_deg: next });
  }

  updateSocketScale(val: number): void {
    const sock = this.charState.selectedSocket();
    if (!sock || !Number.isFinite(val)) return;
    this.charState.updateSocket(sock.id, { scale: Math.max(0.05, val) });
  }

  onAddSocketPart(socketId: string, ev: Event): void {
    const sel = ev.target as HTMLSelectElement;
    const partId = sel.value;
    if (!partId) return;
    this.charState.addSocketPart(socketId, partId);
    sel.value = '';
  }

  updatePartOffset(partId: string, axis: 0 | 1 | 2, val: number): void {
    const part = this.charState.equippedPartsOnSelectedSocket().find((p) => p.id === partId);
    if (!part || part.attachment.kind !== 'socket' || !Number.isFinite(val)) return;
    const next: [number, number, number] = [...part.attachment.offset];
    next[axis] = val;
    this.charState.updatePartSocketTransform(part.id, { offset: next });
  }

  private buildImportTarget(): ImportCharacterGlbTarget {
    const skel = this.charState.activeSkeleton();
    const kind = this.importKind();
    if (kind === 'skinned') {
      const slot =
        skel?.skinned_slots.some((s) => s.id === this.importSlot())
          ? this.importSlot()
          : (skel?.skinned_slots[0]?.id ?? 'torso');
      return { kind: 'skinned', slot, category: this.importCategory().trim() || 'Custom' };
    }
    if (kind === 'socket') {
      const socketId =
        skel?.sockets.some((s) => s.id === this.importSocket())
          ? this.importSocket()
          : (skel?.sockets[0]?.id ?? 'socket_head');
      return { kind: 'socket', socketId, category: this.importCategory().trim() || 'Custom' };
    }
    if (kind === 'animation') {
      return { kind: 'animation' };
    }
    return { kind: 'skeleton', name: this.importSkeletonName().trim() || undefined };
  }

  async onGlbSelected(ev: Event): Promise<void> {
    const input = ev.target as HTMLInputElement;
    if (input.files && input.files.length > 0) {
      await this.charState.importCharacterGlbFiles(input.files, this.buildImportTarget());
      input.value = '';
    }
  }

  onDragOver(ev: DragEvent): void {
    ev.preventDefault();
    this.dragOver.set(true);
  }

  onDragLeave(ev: DragEvent): void {
    ev.preventDefault();
    this.dragOver.set(false);
  }

  async onDrop(ev: DragEvent): Promise<void> {
    ev.preventDefault();
    this.dragOver.set(false);
    if (ev.dataTransfer?.files && ev.dataTransfer.files.length > 0) {
      await this.charState.importCharacterGlbFiles(
        ev.dataTransfer.files,
        this.buildImportTarget(),
      );
    }
  }
}
