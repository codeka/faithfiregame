import { Component, computed, inject } from '@angular/core';
import { DEFAULT_ROAD_MATERIALS, EditorEngineService } from '../../editor-engine.service';
import { EditorStateService } from '../../editor-state.service';

export const ROAD_SUBMODES = [
  { id: 'place', label: 'Place', shortcut: '1' },
  { id: 'edit', label: 'Edit', shortcut: '2' },
] as const;

export const ROAD_STYLES = [
  { id: 'curve', label: 'Curve' },
  { id: 'straight', label: 'Straight' },
] as const;

export const ROAD_MATERIALS = DEFAULT_ROAD_MATERIALS;

@Component({
  selector: 'app-road-tool',
  standalone: true,
  imports: [],
  templateUrl: './road-tool.component.html',
  styleUrls: ['../../editor-controls.css', './road-tool.component.css'],
})
export class RoadToolComponent {
  readonly engine = inject(EditorEngineService);
  readonly state = inject(EditorStateService);

  readonly submodes = ROAD_SUBMODES;
  readonly styles = ROAD_STYLES;
  readonly materials = this.state.roadMaterials;

  readonly activeSubmode = computed(() => this.engine.status()?.roadSubmode ?? 'place');
  readonly draftPoints = computed(() => this.engine.status()?.roadDraftPoints ?? 0);
  readonly selectedRoad = computed(() => this.engine.status()?.selectedRoad ?? null);

  readonly activeStyle = computed(
    () =>
      (this.activeSubmode() === 'edit' ? this.selectedRoad()?.style : undefined) ??
      this.engine.status()?.roadStyle ??
      'curve',
  );
  readonly activeMaterial = computed(
    () =>
      (this.activeSubmode() === 'edit' ? this.selectedRoad()?.material : undefined) ??
      this.engine.status()?.roadMaterial ??
      1,
  );
  readonly activeWidth = computed(
    () =>
      (this.activeSubmode() === 'edit' ? this.selectedRoad()?.width : undefined) ??
      this.engine.status()?.roadWidth ??
      4.0,
  );
  readonly activeShoulder = computed(
    () =>
      (this.activeSubmode() === 'edit' ? this.selectedRoad()?.shoulder : undefined) ??
      this.engine.status()?.roadShoulder ??
      3.0,
  );

  readonly controlsDisabled = computed(
    () => this.activeSubmode() === 'edit' && this.selectedRoad() === null,
  );
  readonly canDeleteNode = computed(() => {
    const sel = this.selectedRoad();
    return sel !== null && sel.selectedNode !== null && sel.pointCount > 2;
  });

  onWidthInput(ev: Event): void {
    const val = Number((ev.target as HTMLInputElement).value);
    if (Number.isFinite(val)) {
      this.engine.setRoadWidth(val);
    }
  }

  onShoulderInput(ev: Event): void {
    const val = Number((ev.target as HTMLInputElement).value);
    if (Number.isFinite(val)) {
      this.engine.setRoadShoulder(val);
    }
  }
}
