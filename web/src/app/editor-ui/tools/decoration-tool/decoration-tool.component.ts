import { Component, computed, inject } from '@angular/core';
import { EditorEngineService } from '../../editor-engine.service';
import { EditorStateService } from '../../editor-state.service';

export const DECORATION_SUBMODES = [
  { id: 'place', label: 'Place', shortcut: '1' },
  { id: 'move', label: 'Move', shortcut: '2' },
  { id: 'free_move', label: 'Free Move', shortcut: '3' },
  { id: 'rotate', label: 'Rotate', shortcut: '4' },
] as const;

@Component({
  selector: 'app-decoration-tool',
  standalone: true,
  imports: [],
  templateUrl: './decoration-tool.component.html',
  styleUrls: ['../../editor-controls.css', './decoration-tool.component.css'],
})
export class DecorationToolComponent {
  readonly engine = inject(EditorEngineService);
  readonly state = inject(EditorStateService);

  readonly submodes = DECORATION_SUBMODES;
  readonly activeSubmode = computed(() => this.engine.status()?.decorationSubmode ?? 'place');
  readonly selectedProp = computed(() => this.engine.status()?.selectedProp ?? null);
  readonly previewYawDeg = computed(() => {
    const deg = this.engine.status()?.previewRotationDeg[1] ?? 0;
    return ((Math.round(deg) % 360) + 360) % 360;
  });

  readonly rotationAxes: readonly {
    axis: 0 | 1 | 2;
    label: string;
    name: string;
    accentClass: string;
  }[] = [
    { axis: 0, label: 'Angle X', name: 'propRotationX', accentClass: 'axis-x' },
    { axis: 1, label: 'Angle Y', name: 'propRotationY', accentClass: 'axis-y' },
    { axis: 2, label: 'Angle Z', name: 'propRotationZ', accentClass: 'axis-z' },
  ];

  readonly activeRotationDeg = computed<[number, number, number]>(() => {
    const sel = this.selectedProp();
    if (!sel) return [0, 0, 0];
    const norm = (deg: number) => ((Math.round(deg) % 360) + 360) % 360;
    return [norm(sel.rotationDeg[0]), norm(sel.rotationDeg[1]), norm(sel.rotationDeg[2])];
  });

  readonly activeScale = computed(() => {
    const sel = this.selectedProp();
    if (!sel) return 1;
    return Number(sel.scale.toFixed(2));
  });

  readonly activeCastShadow = computed(() => {
    const sel = this.selectedProp();
    if (!sel) return true;
    return sel.castShadow;
  });

  onRotationAxisInput(axis: 0 | 1 | 2, ev: Event): void {
    if (!this.selectedProp()) return;
    this.engine.setPropRotationAxisDeg(axis, (ev.target as HTMLInputElement).valueAsNumber);
  }

  onScaleInput(ev: Event): void {
    if (!this.selectedProp()) return;
    this.engine.setPropScale((ev.target as HTMLInputElement).valueAsNumber);
  }

  onCastShadowChange(ev: Event): void {
    if (!this.selectedProp()) return;
    this.engine.setPropCastShadow((ev.target as HTMLInputElement).checked);
  }

  onSearchInput(ev: Event): void {
    this.state.setAssetSearch((ev.target as HTMLInputElement).value);
  }

  clearSearch(): void {
    this.state.setAssetSearch('');
  }

  onSearchEscape(ev: Event): void {
    if (this.state.assetSearch().length > 0) {
      ev.stopPropagation();
      this.state.setAssetSearch('');
    } else {
      (ev.target as HTMLInputElement).blur();
    }
  }
}
