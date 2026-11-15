import { Component, computed, inject } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { EngineService } from '../../../engine.service';
import { EditorStateService } from '../../editor-state.service';

export const HEIGHT_MODES = [
  { id: 'raise', label: 'Raise', shortcut: '1' },
  { id: 'lower', label: 'Lower', shortcut: '2' },
  { id: 'smooth', label: 'Smooth', shortcut: '3' },
  { id: 'flatten', label: 'Flatten', shortcut: '4' },
] as const;

@Component({
  selector: 'app-heightmap-tool',
  standalone: true,
  imports: [FormsModule],
  templateUrl: './heightmap-tool.component.html',
  styleUrls: ['../../editor-controls.css', './heightmap-tool.component.css'],
})
export class HeightmapToolComponent {
  readonly engine = inject(EngineService);
  readonly state = inject(EditorStateService);

  readonly modes = HEIGHT_MODES;

  readonly activeHeightMode = computed(() => this.engine.status()?.heightMode ?? 'raise');
}
