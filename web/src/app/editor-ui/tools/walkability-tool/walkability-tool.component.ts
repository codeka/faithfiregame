import { Component, computed, inject } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { EditorEngineService } from '../../editor-engine.service';
import { EditorStateService } from '../../editor-state.service';

export const WALKABILITY_MODES = [
  { id: 'block', label: 'Block', shortcut: '1' },
  { id: 'walkable', label: 'Walkable', shortcut: '2' },
] as const;

@Component({
  selector: 'app-walkability-tool',
  standalone: true,
  imports: [FormsModule],
  templateUrl: './walkability-tool.component.html',
  styleUrls: ['../../editor-controls.css', './walkability-tool.component.css'],
})
export class WalkabilityToolComponent {
  readonly engine = inject(EditorEngineService);
  readonly state = inject(EditorStateService);

  readonly modes = WALKABILITY_MODES;
  readonly activeMode = computed(() => this.engine.status()?.walkabilityMode ?? 'block');
}
