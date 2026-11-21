import { computed, inject, Injectable, signal } from '@angular/core';
import {
  CharacterCatalog,
  CharacterDefinition,
  CharacterPalette,
  CharacterPartDef,
  EditorEngineService,
  SkeletonDef,
  SocketDef,
} from './editor-engine.service';

export type ImportCharacterGlbTarget =
  | { kind: 'skinned'; slot: string; category: string; name?: string }
  | { kind: 'socket'; socketId: string; category: string; name?: string }
  | { kind: 'animation' }
  | { kind: 'skeleton'; name?: string };

interface UploadedCharacterAsset {
  slug: string;
  name: string;
  path: string;
}

async function readBlobBytes(blob: Blob): Promise<Uint8Array<ArrayBuffer>> {
  if (typeof blob.arrayBuffer === 'function') {
    return new Uint8Array(await blob.arrayBuffer());
  }
  return new Promise<Uint8Array<ArrayBuffer>>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(new Uint8Array(reader.result as ArrayBuffer));
    reader.onerror = () => reject(reader.error ?? new Error('Failed reading file'));
    reader.readAsArrayBuffer(blob);
  });
}

function sanitizeSlug(raw: string, fallback = 'character_item'): string {
  const stem = raw.replace(/\.glb$/i, '').trim();
  const slug = stem
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

export function rgbToHex(rgb: [number, number, number] | null | undefined, fallback = '#cccccc'): string {
  if (!rgb) return fallback;
  const toHex = (n: number) => Math.max(0, Math.min(255, Math.round(n))).toString(16).padStart(2, '0');
  return `#${toHex(rgb[0])}${toHex(rgb[1])}${toHex(rgb[2])}`;
}

export function hexToRgb(hex: string): [number, number, number] | null {
  const clean = hex.trim().replace(/^#/, '');
  if (!/^[0-9a-fA-F]{6}$/.test(clean)) {
    return null;
  }
  return [
    parseInt(clean.slice(0, 2), 16),
    parseInt(clean.slice(2, 4), 16),
    parseInt(clean.slice(4, 6), 16),
  ];
}

function makeDefaultCharacter(skeletonId = 'humanoid'): CharacterDefinition {
  return {
    id: 'custom_character',
    name: 'Custom Character',
    skeleton_id: skeletonId,
    show_base_mesh: true,
    skinned_parts: {},
    socket_parts: {},
    palette: {},
  };
}

@Injectable()
export class CharacterEditorStateService {
  private readonly engine = inject(EditorEngineService);

  readonly catalog = signal<CharacterCatalog>({ skeletons: [], parts: [] });
  readonly presets = signal<CharacterDefinition[]>([]);
  readonly activeCharacter = signal<CharacterDefinition>(makeDefaultCharacter());
  readonly selectedSubTab = signal<'assemble' | 'rig'>('assemble');
  readonly selectedSocketId = signal<string>('');
  readonly saveState = signal<string>('Saved');
  readonly catalogDirty = signal<boolean>(false);
  readonly presetDirty = signal<boolean>(false);

  readonly importingGlb = signal<boolean>(false);
  readonly importMessage = signal<string | null>(null);
  readonly importError = signal<string | null>(null);

  readonly activeSkeleton = computed<SkeletonDef | null>(() => {
    const skels = this.catalog().skeletons;
    if (skels.length === 0) return null;
    const targetId = this.activeCharacter().skeleton_id;
    return skels.find((s) => s.id === targetId) ?? skels[0];
  });

  readonly partsForActiveSkeleton = computed<readonly CharacterPartDef[]>(() => {
    const skelId = this.activeSkeleton()?.id;
    if (!skelId) return [];
    return this.catalog().parts.filter((p) => p.skeleton_id === skelId);
  });

  readonly selectedSocket = computed<SocketDef | null>(() => {
    const skel = this.activeSkeleton();
    if (!skel || skel.sockets.length === 0) return null;
    const id = this.selectedSocketId();
    return skel.sockets.find((s) => s.id === id) ?? skel.sockets[0];
  });

  readonly equippedPartOnSelectedSocket = computed<CharacterPartDef | null>(() => {
    const sock = this.selectedSocket();
    if (!sock) return null;
    const partId = this.activeCharacter().socket_parts[sock.id];
    if (!partId) return null;
    return this.catalog().parts.find((p) => p.id === partId) ?? null;
  });

  async initData(): Promise<void> {
    await Promise.all([this.loadCatalog(), this.loadPresets()]);
    const skels = this.catalog().skeletons;
    const list = this.presets();
    if (list.length > 0) {
      const starter = list.find((p) => p.id === 'cryo_prisoner') ?? list[0];
      this.applyActiveCharacter(starter, false);
    } else if (skels.length > 0) {
      this.applyActiveCharacter(makeDefaultCharacter(skels[0].id), false);
    }
    const firstSock = this.activeSkeleton()?.sockets[0]?.id ?? '';
    if (firstSock && !this.selectedSocketId()) {
      this.selectSocket(firstSock);
    }
  }

  async loadCatalog(): Promise<void> {
    try {
      const resp = await fetch('/api/editor/characters/catalog', { cache: 'no-cache' });
      if (resp.ok) {
        const cat = (await resp.json()) as CharacterCatalog;
        const normalized: CharacterCatalog = {
          skeletons: cat.skeletons ?? [],
          parts: cat.parts ?? [],
        };
        this.catalog.set(normalized);
        this.engine.setCharacterCatalog(normalized);
      }
    } catch (e) {
      console.error('Failed to load character catalog:', e);
    }
  }

  async loadPresets(): Promise<void> {
    try {
      const resp = await fetch('/api/editor/characters/presets', { cache: 'no-cache' });
      if (resp.ok) {
        const list = (await resp.json()) as CharacterDefinition[];
        this.presets.set(list);
        this.engine.setCharacterPresets(list);
      }
    } catch (e) {
      console.error('Failed to load character presets:', e);
    }
  }

  private applyActiveCharacter(def: CharacterDefinition, dirty: boolean): void {
    const clone: CharacterDefinition = {
      ...def,
      skinned_parts: { ...(def.skinned_parts ?? {}) },
      socket_parts: { ...(def.socket_parts ?? {}) },
      palette: { ...(def.palette ?? {}) },
    };
    this.activeCharacter.set(clone);
    this.presetDirty.set(dirty);
    this.refreshSaveBadge();
    this.engine.setActiveCharacter(clone);
    const firstSock = this.activeSkeleton()?.sockets[0]?.id ?? '';
    if (
      firstSock &&
      (!this.selectedSocketId() ||
        !this.activeSkeleton()?.sockets.some((s) => s.id === this.selectedSocketId()))
    ) {
      this.selectSocket(firstSock);
    }
  }

  private refreshSaveBadge(): void {
    this.saveState.set(this.presetDirty() || this.catalogDirty() ? 'Unsaved' : 'Saved');
  }

  partsForSlot(slotId: string): CharacterPartDef[] {
    return this.partsForActiveSkeleton().filter(
      (p) => p.attachment.kind === 'skinned' && p.attachment.slot === slotId,
    );
  }

  partsForSocket(socketId: string): CharacterPartDef[] {
    return this.partsForActiveSkeleton().filter(
      (p) => p.attachment.kind === 'socket' && p.attachment.socket_id === socketId,
    );
  }

  equippedSkinnedPart(slotId: string): string {
    return this.activeCharacter().skinned_parts[slotId] ?? '';
  }

  equippedSocketPart(socketId: string): string {
    return this.activeCharacter().socket_parts[socketId] ?? '';
  }

  selectPreset(presetId: string): void {
    const found = this.presets().find((p) => p.id === presetId);
    if (found) {
      this.applyActiveCharacter(found, false);
    }
  }

  private uniquePresetId(baseSlug: string): string {
    const existing = new Set(this.presets().map((p) => p.id));
    if (!existing.has(baseSlug)) return baseSlug;
    let n = 2;
    while (existing.has(`${baseSlug}_${n}`)) {
      n += 1;
    }
    return `${baseSlug}_${n}`;
  }

  createNewPreset(name = 'New Character'): void {
    const id = this.uniquePresetId(sanitizeSlug(name, 'new_character'));
    const skelId = this.activeSkeleton()?.id ?? 'humanoid';
    const created: CharacterDefinition = {
      id,
      name,
      skeleton_id: skelId,
      show_base_mesh: true,
      skinned_parts: {},
      socket_parts: {},
      palette: {},
    };
    this.presets.update((list) => [...list, created]);
    this.engine.setCharacterPresets(this.presets());
    this.applyActiveCharacter(created, true);
  }

  duplicatePreset(): void {
    const cur = this.activeCharacter();
    const id = this.uniquePresetId(`${cur.id}_copy`);
    const dup: CharacterDefinition = {
      ...cur,
      id,
      name: `${cur.name} Copy`,
      skinned_parts: { ...cur.skinned_parts },
      socket_parts: { ...cur.socket_parts },
      palette: { ...cur.palette },
    };
    this.presets.update((list) => [...list, dup]);
    this.engine.setCharacterPresets(this.presets());
    this.applyActiveCharacter(dup, true);
  }

  async deleteActivePreset(): Promise<void> {
    const cur = this.activeCharacter();
    try {
      const resp = await fetch(`/api/editor/characters/presets/${encodeURIComponent(cur.id)}`, {
        method: 'DELETE',
      });
      if (!resp.ok && resp.status !== 404) {
        throw new Error(`Delete failed (HTTP ${resp.status})`);
      }
      const nextList = this.presets().filter((p) => p.id !== cur.id);
      this.presets.set(nextList);
      this.engine.setCharacterPresets(nextList);
      if (nextList.length > 0) {
        this.applyActiveCharacter(nextList[0], false);
      } else {
        this.createNewPreset('New Character');
      }
    } catch (e) {
      console.error('Failed deleting character preset:', e);
      this.saveState.set('Save failed');
    }
  }

  setCharacterName(name: string): void {
    this.activeCharacter.update((c) => ({ ...c, name }));
    this.presetDirty.set(true);
    this.refreshSaveBadge();
    this.engine.setActiveCharacter(this.activeCharacter());
  }

  setSkeletonId(skeletonId: string): void {
    const current = this.activeCharacter();
    if (current.skeleton_id === skeletonId) return;
    const validParts = new Set(
      this.catalog()
        .parts.filter((p) => p.skeleton_id === skeletonId)
        .map((p) => p.id),
    );
    const skinned_parts: Record<string, string> = {};
    for (const [k, v] of Object.entries(current.skinned_parts)) {
      if (validParts.has(v)) skinned_parts[k] = v;
    }
    const socket_parts: Record<string, string> = {};
    for (const [k, v] of Object.entries(current.socket_parts)) {
      if (validParts.has(v)) socket_parts[k] = v;
    }
    this.applyActiveCharacter(
      { ...current, skeleton_id: skeletonId, skinned_parts, socket_parts },
      true,
    );
  }

  setShowBaseMesh(show: boolean): void {
    this.activeCharacter.update((c) => ({ ...c, show_base_mesh: show }));
    this.presetDirty.set(true);
    this.refreshSaveBadge();
    this.engine.setActiveCharacter(this.activeCharacter());
  }

  setSkinnedPart(slotId: string, partId: string): void {
    this.activeCharacter.update((c) => {
      const next = { ...c.skinned_parts };
      if (!partId) {
        delete next[slotId];
      } else {
        next[slotId] = partId;
      }
      return { ...c, skinned_parts: next };
    });
    this.presetDirty.set(true);
    this.refreshSaveBadge();
    this.engine.setActiveCharacter(this.activeCharacter());
  }

  setSocketPart(socketId: string, partId: string): void {
    this.activeCharacter.update((c) => {
      const next = { ...c.socket_parts };
      if (!partId) {
        delete next[socketId];
      } else {
        next[socketId] = partId;
      }
      return { ...c, socket_parts: next };
    });
    this.presetDirty.set(true);
    this.refreshSaveBadge();
    this.engine.setActiveCharacter(this.activeCharacter());
  }

  setPaletteColor(key: keyof CharacterPalette, hex: string): void {
    const rgb = hexToRgb(hex);
    if (!rgb) return;
    this.activeCharacter.update((c) => ({
      ...c,
      palette: { ...c.palette, [key]: rgb },
    }));
    this.presetDirty.set(true);
    this.refreshSaveBadge();
    this.engine.setActiveCharacter(this.activeCharacter());
  }

  clearPaletteColor(key: keyof CharacterPalette): void {
    this.activeCharacter.update((c) => {
      const nextPalette = { ...c.palette };
      delete nextPalette[key];
      return { ...c, palette: nextPalette };
    });
    this.presetDirty.set(true);
    this.refreshSaveBadge();
    this.engine.setActiveCharacter(this.activeCharacter());
  }

  selectSocket(socketId: string): void {
    this.selectedSocketId.set(socketId);
    this.engine.setSelectedSocketId(socketId || null);
  }

  private updateCatalogState(next: CharacterCatalog, dirty = true): void {
    this.catalog.set(next);
    if (dirty) {
      this.catalogDirty.set(true);
      this.refreshSaveBadge();
    }
    this.engine.setCharacterCatalog(next);
  }

  addSkinnedSlot(rawId: string, label: string): void {
    const skel = this.activeSkeleton();
    if (!skel) return;
    const id = sanitizeSlug(rawId || label, 'custom_slot');
    if (skel.skinned_slots.some((s) => s.id === id)) return;
    const slotName = label.trim() || prettifySlug(id);
    const nextSkels = this.catalog().skeletons.map((s) =>
      s.id === skel.id
        ? { ...s, skinned_slots: [...s.skinned_slots, { id, name: slotName }] }
        : s,
    );
    this.updateCatalogState({ ...this.catalog(), skeletons: nextSkels });
  }

  addSocket(rawId: string, label: string, parentBone: string): void {
    const skel = this.activeSkeleton();
    if (!skel) return;
    const base = sanitizeSlug(rawId || label, 'socket_custom');
    const id = base.startsWith('socket_') ? base : `socket_${base}`;
    if (skel.sockets.some((s) => s.id === id)) {
      this.selectSocket(id);
      return;
    }
    const newSock: SocketDef = {
      id,
      name: label.trim() || prettifySlug(id),
      parent_bone: parentBone || 'head',
      offset: [0, 0, 0],
      rotation_deg: [0, 0, 0],
      scale: 1,
    };
    const nextSkels = this.catalog().skeletons.map((s) =>
      s.id === skel.id ? { ...s, sockets: [...s.sockets, newSock] } : s,
    );
    this.updateCatalogState({ ...this.catalog(), skeletons: nextSkels });
    this.selectSocket(id);
  }

  updateSocket(socketId: string, patch: Partial<SocketDef>): void {
    const skel = this.activeSkeleton();
    if (!skel) return;
    const nextSkels = this.catalog().skeletons.map((s) =>
      s.id === skel.id
        ? {
            ...s,
            sockets: s.sockets.map((sock) =>
              sock.id === socketId ? { ...sock, ...patch } : sock,
            ),
          }
        : s,
    );
    this.updateCatalogState({ ...this.catalog(), skeletons: nextSkels });
  }

  updatePartSocketTransform(
    partId: string,
    patch: {
      offset?: [number, number, number];
      rotation_deg?: [number, number, number];
      scale?: number;
    },
  ): void {
    const nextParts = this.catalog().parts.map((p) => {
      if (p.id !== partId || p.attachment.kind !== 'socket') return p;
      return {
        ...p,
        attachment: {
          ...p.attachment,
          ...patch,
        },
      };
    });
    this.updateCatalogState({ ...this.catalog(), parts: nextParts });
  }

  async importCharacterGlbFiles(
    files: FileList | readonly File[],
    target: ImportCharacterGlbTarget,
  ): Promise<void> {
    const glbFiles = Array.from(files).filter((f) => f.name.toLowerCase().endsWith('.glb'));
    if (glbFiles.length === 0) {
      this.importError.set('Select one or more binary .glb files.');
      this.importMessage.set(null);
      return;
    }

    this.importingGlb.set(true);
    this.importError.set(null);
    this.importMessage.set(null);

    let count = 0;
    try {
      for (const file of glbFiles) {
        const slug = sanitizeSlug(file.name, 'character_asset');
        const bytes = await readBlobBytes(file);
        const resp = await fetch(`/api/editor/characters/assets/${encodeURIComponent(slug)}`, {
          method: 'PUT',
          headers: { 'Content-Type': 'model/gltf-binary' },
          body: file,
        });
        if (!resp.ok) {
          const text = await resp.text();
          throw new Error(text || `Upload failed (HTTP ${resp.status})`);
        }
        const uploaded = (await resp.json()) as UploadedCharacterAsset;
        const cat = this.catalog();
        const skelId = this.activeSkeleton()?.id ?? cat.skeletons[0]?.id ?? 'humanoid';

        if (target.kind === 'skeleton') {
          const newSkel: SkeletonDef = {
            id: uploaded.slug,
            name: target.name?.trim() || uploaded.name,
            rig_path: uploaded.path,
            render_base_mesh: true,
            skinned_slots: [
              { id: 'head', name: 'Head' },
              { id: 'torso', name: 'Torso' },
              { id: 'legs', name: 'Legs' },
            ],
            sockets: [],
            animation_paths: [],
          };
          const filtered = cat.skeletons.filter((s) => s.id !== newSkel.id);
          this.updateCatalogState({ ...cat, skeletons: [...filtered, newSkel] });
          this.engine.loadCharacterGlbBytes(uploaded.path, bytes);
          this.setSkeletonId(newSkel.id);
        } else if (target.kind === 'animation') {
          const nextSkels = cat.skeletons.map((s) =>
            s.id === skelId && !s.animation_paths.includes(uploaded.path)
              ? { ...s, animation_paths: [...s.animation_paths, uploaded.path] }
              : s,
          );
          this.updateCatalogState({ ...cat, skeletons: nextSkels });
          this.engine.loadCharacterGlbBytes(uploaded.path, bytes);
        } else if (target.kind === 'skinned') {
          const newPart: CharacterPartDef = {
            id: uploaded.slug,
            name: target.name?.trim() || uploaded.name,
            skeleton_id: skelId,
            attachment: { kind: 'skinned', slot: target.slot },
            path: uploaded.path,
            category: target.category || prettifySlug(target.slot),
          };
          const filtered = cat.parts.filter((p) => p.id !== newPart.id);
          this.updateCatalogState({ ...cat, parts: [...filtered, newPart] });
          this.engine.loadCharacterGlbBytes(uploaded.path, bytes);
          this.setSkinnedPart(target.slot, newPart.id);
        } else {
          const newPart: CharacterPartDef = {
            id: uploaded.slug,
            name: target.name?.trim() || uploaded.name,
            skeleton_id: skelId,
            attachment: {
              kind: 'socket',
              socket_id: target.socketId,
              offset: [0, 0, 0],
              rotation_deg: [0, 0, 0],
              scale: 1,
            },
            path: uploaded.path,
            category: target.category || 'Socket',
          };
          const filtered = cat.parts.filter((p) => p.id !== newPart.id);
          this.updateCatalogState({ ...cat, parts: [...filtered, newPart] });
          this.engine.loadCharacterGlbBytes(uploaded.path, bytes);
          this.setSocketPart(target.socketId, newPart.id);
        }
        count += 1;
      }

      this.importMessage.set(
        count === 1
          ? `Imported ${glbFiles[0].name}`
          : `Imported ${count} character .glb files`,
      );
    } catch (e) {
      this.importError.set(e instanceof Error ? e.message : String(e));
    } finally {
      this.importingGlb.set(false);
    }
  }

  async saveCatalog(): Promise<boolean> {
    const resp = await fetch('/api/editor/characters/catalog', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(this.catalog()),
    });
    if (!resp.ok) {
      return false;
    }
    const saved = (await resp.json()) as CharacterCatalog;
    this.catalog.set(saved);
    this.catalogDirty.set(false);
    this.engine.setCharacterCatalog(saved);
    return true;
  }

  async saveActivePreset(): Promise<boolean> {
    const def = this.activeCharacter();
    const resp = await fetch(`/api/editor/characters/presets/${encodeURIComponent(def.id)}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(def),
    });
    if (!resp.ok) {
      return false;
    }
    const saved = (await resp.json()) as CharacterDefinition;
    this.activeCharacter.set(saved);
    this.presets.update((list) => {
      const idx = list.findIndex((p) => p.id === saved.id);
      if (idx >= 0) {
        const copy = [...list];
        copy[idx] = saved;
        return copy;
      }
      return [...list, saved].sort((a, b) => a.id.localeCompare(b.id));
    });
    this.presetDirty.set(false);
    this.engine.setCharacterPresets(this.presets());
    this.engine.setActiveCharacter(saved);
    return true;
  }

  async saveAll(): Promise<void> {
    this.saveState.set('Saving...');
    try {
      if (this.catalogDirty()) {
        const okCat = await this.saveCatalog();
        if (!okCat) {
          this.saveState.set('Save failed');
          return;
        }
      }
      const okPreset = await this.saveActivePreset();
      if (!okPreset) {
        this.saveState.set('Save failed');
        return;
      }
      this.refreshSaveBadge();
    } catch (e) {
      console.error('Failed saving character changes:', e);
      this.saveState.set('Save failed');
    }
  }
}
