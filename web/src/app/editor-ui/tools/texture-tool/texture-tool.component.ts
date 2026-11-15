import { Component, computed, inject } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { EngineService } from '../../../engine.service';
import { EditorStateService } from '../../editor-state.service';

export const TEXTURE_LAYERS = [
  { index: 0, label: 'Grass', swatch: '#4a8f3d', shortcut: '1' },
  { index: 1, label: 'Dirt', swatch: '#8b623f', shortcut: '2' },
  { index: 2, label: 'Rock', swatch: '#787e88', shortcut: '3' },
  { index: 3, label: 'Sand', swatch: '#d7be7a', shortcut: '4' },
] as const;

@Component({
  selector: 'app-texture-tool',
  standalone: true,
  imports: [FormsModule],
  templateUrl: './texture-tool.component.html',
  styleUrls: ['../../editor-controls.css', './texture-tool.component.css'],
})
export class TextureToolComponent {
  readonly engine = inject(EngineService);
  readonly state = inject(EditorStateService);

  readonly activeTexLayer = computed(() => this.engine.status()?.textureLayer ?? 1);

  readonly textureLayers = TEXTURE_LAYERS;
}
