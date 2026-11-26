import {
  AfterViewInit,
  Component,
  computed,
  ElementRef,
  inject,
  input,
  OnDestroy,
  output,
  signal,
  viewChild,
} from '@angular/core';
import { FormsModule } from '@angular/forms';
import {
  EngineService,
  EquipSlot,
  GamePlayerInventory,
  PlayerInventoryItemView,
} from '../../../engine.service';
import { HudButtonComponent } from '../../primitives/hud-button/hud-button.component';
import { HudFrameComponent } from '../../primitives/hud-frame/hud-frame.component';

interface EquipSlotMeta {
  slot: EquipSlot;
  label: string;
}

interface ResolvedEquipSlot {
  slot: EquipSlot;
  label: string;
  item: PlayerInventoryItemView | null;
  isTwoHandedGhost: boolean;
}

type DragSource =
  | { kind: 'bag'; slot: number; item: PlayerInventoryItemView }
  | { kind: 'equip'; slot: EquipSlot; item: PlayerInventoryItemView };

interface SplitDraft {
  fromSlot: number;
  toSlot: number;
  itemName: string;
  maxSplit: number;
  amount: number;
}

const LEFT_EQUIP_SLOTS: readonly EquipSlotMeta[] = [
  { slot: 'head', label: 'Head' },
  { slot: 'body', label: 'Body' },
  { slot: 'legs', label: 'Legs' },
  { slot: 'feet', label: 'Feet' },
];

const RIGHT_EQUIP_SLOTS: readonly EquipSlotMeta[] = [
  { slot: 'hands', label: 'Hands' },
  { slot: 'back', label: 'Back' },
  { slot: 'main_hand', label: 'Main Hand' },
  { slot: 'off_hand', label: 'Off Hand' },
];

const EQUIP_SLOT_LABEL: Record<EquipSlot, string> = {
  head: 'Head',
  body: 'Body',
  legs: 'Legs',
  feet: 'Feet',
  hands: 'Hands',
  back: 'Back',
  main_hand: 'Main Hand',
  off_hand: 'Off Hand',
};

const PREVIEW_W = 192;
const PREVIEW_H = 276;

@Component({
  selector: 'app-hud-inventory-panel',
  standalone: true,
  imports: [FormsModule, HudButtonComponent, HudFrameComponent],
  templateUrl: './inventory-panel.component.html',
  styleUrl: './inventory-panel.component.css',
})
export class InventoryPanelComponent implements AfterViewInit, OnDestroy {
  private readonly engine = inject(EngineService, { optional: true });

  readonly inventory = input.required<GamePlayerInventory>();
  readonly thumbnails = input<Record<string, string>>({});

  readonly close = output<void>();
  readonly equipItem = output<{ bagSlot: number; targetSlot: EquipSlot | null }>();
  readonly unequipSlot = output<{ slot: EquipSlot; targetBagSlot: number | null }>();
  readonly moveItem = output<{ fromSlot: number; toSlot: number }>();
  readonly splitStack = output<{ fromSlot: number; toSlot: number; count: number }>();

  readonly previewCanvas = viewChild<ElementRef<HTMLCanvasElement>>('previewCanvas');

  readonly previewYaw = signal(0);
  readonly dragSource = signal<DragSource | null>(null);
  readonly dropHoverTarget = signal<string | null>(null);
  readonly hoveredInfo = signal<{ item: PlayerInventoryItemView; isTwoHandedGhost: boolean } | null>(
    null,
  );
  readonly splitDraft = signal<SplitDraft | null>(null);

  private rafId: number | null = null;
  private draggingPreview = false;
  private lastPointerX = 0;

  readonly equippedBySlot = computed<Partial<Record<EquipSlot, PlayerInventoryItemView>>>(() => {
    const map: Partial<Record<EquipSlot, PlayerInventoryItemView>> = {};
    for (const item of this.inventory().items) {
      if (item.equipped && item.count > 0 && item.equipSlot) {
        map[item.equipSlot] = item;
      }
    }
    return map;
  });

  readonly leftEquipSlots = computed<readonly ResolvedEquipSlot[]>(() =>
    this.resolveColumnSlots(LEFT_EQUIP_SLOTS),
  );

  readonly rightEquipSlots = computed<readonly ResolvedEquipSlot[]>(() =>
    this.resolveColumnSlots(RIGHT_EQUIP_SLOTS),
  );

  readonly bagSlots = computed<readonly (PlayerInventoryItemView | null)[]>(() => {
    const inv = this.inventory();
    const cap = Math.max(0, inv.bagCapacity);
    const cells: (PlayerInventoryItemView | null)[] = Array.from({ length: cap }, () => null);
    const overflow: PlayerInventoryItemView[] = [];
    for (const item of inv.items) {
      if (item.equipped || item.count <= 0) {
        continue;
      }
      if (item.bagSlot !== null && item.bagSlot >= 0 && item.bagSlot < cap && !cells[item.bagSlot]) {
        cells[item.bagSlot] = item;
      } else {
        overflow.push(item);
      }
    }
    for (const item of overflow) {
      const idx = cells.indexOf(null);
      if (idx >= 0) {
        cells[idx] = item;
      }
    }
    return cells;
  });

  readonly usedBagCount = computed(() => this.bagSlots().filter(Boolean).length);

  readonly weightState = computed<'cyan' | 'amber' | 'crimson'>(() => {
    const inv = this.inventory();
    if (inv.maxWeight <= 0) return 'cyan';
    const ratio = inv.totalWeight / inv.maxWeight;
    if (ratio >= 0.999) return 'crimson';
    if (ratio >= 0.85) return 'amber';
    return 'cyan';
  });

  ngAfterViewInit(): void {
    const tick = () => {
      this.drawLivePreview();
      if (typeof requestAnimationFrame === 'function') {
        this.rafId = requestAnimationFrame(tick);
      }
    };
    this.drawLivePreview();
    if (typeof requestAnimationFrame === 'function') {
      this.rafId = requestAnimationFrame(tick);
    }
  }

  ngOnDestroy(): void {
    if (this.rafId !== null && typeof cancelAnimationFrame === 'function') {
      cancelAnimationFrame(this.rafId);
      this.rafId = null;
    }
  }

  private drawLivePreview(): void {
    const canvas = this.previewCanvas()?.nativeElement;
    if (!canvas || !this.engine) return;
    const rgba = this.engine.renderCharacterPreview(PREVIEW_W, PREVIEW_H, this.previewYaw());
    if (!rgba || rgba.length !== PREVIEW_W * PREVIEW_H * 4) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const imgData = new ImageData(new Uint8ClampedArray(rgba), PREVIEW_W, PREVIEW_H);
    ctx.putImageData(imgData, 0, 0);
  }

  private resolveColumnSlots(defs: readonly EquipSlotMeta[]): ResolvedEquipSlot[] {
    const map = this.equippedBySlot();
    const mainHand = map.main_hand ?? null;
    return defs.map(({ slot, label }) => {
      const direct = map[slot] ?? null;
      if (!direct && slot === 'off_hand' && mainHand?.twoHanded) {
        return { slot, label, item: mainHand, isTwoHandedGhost: true };
      }
      return { slot, label, item: direct, isTwoHandedGhost: false };
    });
  }

  thumbFor(item: PlayerInventoryItemView | null): string | null {
    const glb = item?.previewGlb;
    if (!glb) return null;
    return this.thumbnails()[glb] ?? null;
  }

  slotLabel(slot: EquipSlot | null): string {
    return slot ? EQUIP_SLOT_LABEL[slot] : '';
  }

  onPreviewPointerDown(ev: PointerEvent): void {
    if (ev.button !== 0) return;
    this.draggingPreview = true;
    this.lastPointerX = ev.clientX;
    const el = ev.currentTarget as HTMLElement | null;
    if (el && typeof el.setPointerCapture === 'function') {
      try {
        el.setPointerCapture(ev.pointerId);
      } catch {
        // Ignore in synthetic test environments.
      }
    }
  }

  onPreviewPointerMove(ev: PointerEvent): void {
    if (!this.draggingPreview) return;
    const dx = ev.clientX - this.lastPointerX;
    this.lastPointerX = ev.clientX;
    this.previewYaw.update((y) => y + dx * 0.012);
  }

  onPreviewPointerUp(): void {
    this.draggingPreview = false;
  }

  onPreviewDblClick(): void {
    this.previewYaw.set(0);
  }

  canDropOnEquipSlot(slot: EquipSlot): boolean {
    const src = this.dragSource();
    if (!src || src.kind !== 'bag') return false;
    const eq = src.item.equipSlot;
    return eq === slot || (eq === 'main_hand' && src.item.twoHanded && slot === 'off_hand');
  }

  onBagDragStart(slot: number, item: PlayerInventoryItemView, ev: DragEvent): void {
    this.dragSource.set({ kind: 'bag', slot, item });
    if (ev.dataTransfer) {
      ev.dataTransfer.effectAllowed = 'move';
      ev.dataTransfer.setData('text/plain', `bag:${slot}`);
    }
  }

  onEquipDragStart(slot: EquipSlot, item: PlayerInventoryItemView, ev: DragEvent): void {
    this.dragSource.set({ kind: 'equip', slot, item });
    if (ev.dataTransfer) {
      ev.dataTransfer.effectAllowed = 'move';
      ev.dataTransfer.setData('text/plain', `equip:${slot}`);
    }
  }

  onDragEnd(): void {
    this.dragSource.set(null);
    this.dropHoverTarget.set(null);
  }

  onEquipDragOver(slot: EquipSlot, ev: DragEvent): void {
    if (!this.canDropOnEquipSlot(slot)) return;
    ev.preventDefault();
    this.dropHoverTarget.set(`equip:${slot}`);
  }

  onEquipDragLeave(slot: EquipSlot): void {
    if (this.dropHoverTarget() === `equip:${slot}`) {
      this.dropHoverTarget.set(null);
    }
  }

  onEquipDrop(slot: EquipSlot, ev: DragEvent): void {
    ev.preventDefault();
    const src = this.dragSource();
    this.dragSource.set(null);
    this.dropHoverTarget.set(null);
    if (!src || src.kind !== 'bag') return;
    const eq = src.item.equipSlot;
    if (eq === slot || (eq === 'main_hand' && src.item.twoHanded && slot === 'off_hand')) {
      this.equipItem.emit({ bagSlot: src.slot, targetSlot: slot });
    }
  }

  onBagDragOver(toSlot: number, ev: DragEvent): void {
    if (!this.dragSource()) return;
    ev.preventDefault();
    this.dropHoverTarget.set(`bag:${toSlot}`);
  }

  onBagDragLeave(toSlot: number): void {
    if (this.dropHoverTarget() === `bag:${toSlot}`) {
      this.dropHoverTarget.set(null);
    }
  }

  onBagDrop(toSlot: number, ev: DragEvent): void {
    ev.preventDefault();
    const src = this.dragSource();
    this.dragSource.set(null);
    this.dropHoverTarget.set(null);
    if (!src) return;
    if (src.kind === 'bag') {
      if (src.slot === toSlot) return;
      if (ev.shiftKey && src.item.count > 1) {
        const dest = this.bagSlots()[toSlot];
        if (!dest || (dest.itemId === src.item.itemId && dest.count < dest.maxStack)) {
          this.openSplitModal(src.slot, toSlot, src.item);
          return;
        }
      }
      this.moveItem.emit({ fromSlot: src.slot, toSlot });
    } else {
      this.unequipSlot.emit({ slot: src.slot, targetBagSlot: toSlot });
    }
  }

  onBagDblClick(slot: number, item: PlayerInventoryItemView | null): void {
    if (!item || !item.equipSlot) return;
    this.equipItem.emit({ bagSlot: slot, targetSlot: null });
  }

  onEquipDblClick(slot: EquipSlot, item: PlayerInventoryItemView | null): void {
    if (!item) return;
    this.unequipSlot.emit({ slot, targetBagSlot: null });
  }

  onBagContextMenu(slot: number, item: PlayerInventoryItemView | null, ev: MouseEvent): void {
    if (!item || item.count <= 1) return;
    ev.preventDefault();
    const freeSlot = this.bagSlots().findIndex((cell) => cell === null);
    if (freeSlot < 0) return;
    this.openSplitModal(slot, freeSlot, item);
  }

  private openSplitModal(fromSlot: number, toSlot: number, item: PlayerInventoryItemView): void {
    const maxSplit = item.count - 1;
    if (maxSplit < 1) return;
    this.splitDraft.set({
      fromSlot,
      toSlot,
      itemName: item.name,
      maxSplit,
      amount: Math.max(1, Math.floor(item.count / 2)),
    });
  }

  setSplitAmount(raw: number): void {
    const cur = this.splitDraft();
    if (!cur || !Number.isFinite(raw)) return;
    const clamped = Math.max(1, Math.min(cur.maxSplit, Math.round(raw)));
    this.splitDraft.set({ ...cur, amount: clamped });
  }

  confirmSplit(): void {
    const cur = this.splitDraft();
    if (!cur) return;
    this.splitStack.emit({ fromSlot: cur.fromSlot, toSlot: cur.toSlot, count: cur.amount });
    this.splitDraft.set(null);
  }

  cancelSplit(): void {
    this.splitDraft.set(null);
  }

  setHovered(item: PlayerInventoryItemView | null, isTwoHandedGhost = false): void {
    this.hoveredInfo.set(item ? { item, isTwoHandedGhost } : null);
  }
}
