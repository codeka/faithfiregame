import { Component, computed, inject } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { EngineService } from '../../../engine.service';
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
  imports: [FormsModule],
  templateUrl: './decoration-tool.component.html',
  styleUrls: ['../../editor-controls.css', './decoration-tool.component.css'],
})
export class DecorationToolComponent {
  readonly engine = inject(EngineService);
  readonly state = inject(EditorStateService);

  readonly submodes = DECORATION_SUBMODES;
  readonly activeSubmode = computed(() => this.engine.status()?.decorationSubmode ?? 'place');
  readonly selectedProp = computed(() => this.engine.status()?.selectedProp ?? null);

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
    const s = this.engine.status();
    if (!s) return [0, 0, 0];
    const raw = s.selectedProp ? s.selectedProp.rotationDeg : s.previewRotationDeg;
    const norm = (deg: number) => ((Math.round(deg) % 360) + 360) % 360;
    return [norm(raw[0]), norm(raw[1]), norm(raw[2])];
  });

  readonly activeScale = computed(() => {
    const s = this.engine.status();
    if (!s) return 1;
    return Number((s.selectedProp ? s.selectedProp.scale : s.previewScale).toFixed(2));
  });

  onRotationAxisChange(axis: 0 | 1 | 2, deg: number): void {
    this.engine.setPropRotationAxisDeg(axis, Number(deg));
  }

  onScaleChange(scale: number): void {
    this.engine.setPropScale(Number(scale));
  }
}
