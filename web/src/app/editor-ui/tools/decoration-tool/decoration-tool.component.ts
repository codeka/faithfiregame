import { Component, computed, inject } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { EngineService } from '../../../engine.service';
import { EditorStateService } from '../../editor-state.service';

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

  readonly submodes: readonly ('place' | 'move' | 'rotate')[] = ['place', 'move', 'rotate'];
  readonly activeSubmode = computed(() => this.engine.status()?.decorationSubmode ?? 'place');
  readonly selectedProp = computed(() => this.engine.status()?.selectedProp ?? null);

  readonly activeRotationDeg = computed(() => {
    const s = this.engine.status();
    if (!s) return 0;
    return Math.round(s.selectedProp ? s.selectedProp.rotationDeg : s.previewRotationDeg);
  });

  readonly activeScale = computed(() => {
    const s = this.engine.status();
    if (!s) return 1;
    return Number((s.selectedProp ? s.selectedProp.scale : s.previewScale).toFixed(2));
  });

  onRotationChange(deg: number): void {
    this.engine.setPropRotationDeg(Number(deg));
  }

  onScaleChange(scale: number): void {
    this.engine.setPropScale(Number(scale));
  }
}
