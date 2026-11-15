import { Component, computed, inject } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { EngineService } from '../../../engine.service';
import { EditorStateService } from '../../editor-state.service';

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

  readonly modes: readonly ('raise' | 'lower' | 'smooth' | 'flatten')[] = [
    'raise',
    'lower',
    'smooth',
    'flatten',
  ];

  readonly activeHeightMode = computed(() => this.engine.status()?.heightMode ?? 'raise');
}
