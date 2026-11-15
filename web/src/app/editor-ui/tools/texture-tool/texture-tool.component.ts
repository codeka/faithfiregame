import { Component, computed, inject } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { EditorEngineService } from '../../editor-engine.service';
import { EditorStateService } from '../../editor-state.service';

export const TEXTURE_CATEGORIES = ['All', 'Grass', 'Earth', 'Stone'] as const;

export const TEXTURE_LAYERS = [
  { index: 0, label: 'Grass', category: 'Grass', swatch: '#4a8f3d', shortcut: '1' },
  { index: 4, label: 'Light Grass', category: 'Grass', swatch: '#76b546', shortcut: '2' },
  { index: 5, label: 'Dark Grass', category: 'Grass', swatch: '#2a5f32', shortcut: '3' },
  { index: 6, label: 'Patchy Grass', category: 'Grass', swatch: '#6d7b3a', shortcut: '4' },
  { index: 1, label: 'Dirt', category: 'Earth', swatch: '#8b623f', shortcut: '5' },
  { index: 3, label: 'Sand', category: 'Earth', swatch: '#d7be7a', shortcut: '6' },
  { index: 2, label: 'Rock', category: 'Stone', swatch: '#787e88', shortcut: '7' },
  { index: 7, label: 'Cobblestone', category: 'Stone', swatch: '#72757b', shortcut: '8' },
  { index: 8, label: 'Brick', category: 'Stone', swatch: '#9c4c3a', shortcut: '9' },
] as const;

@Component({
  selector: 'app-texture-tool',
  standalone: true,
  imports: [FormsModule],
  templateUrl: './texture-tool.component.html',
  styleUrls: ['../../editor-controls.css', './texture-tool.component.css'],
})
export class TextureToolComponent {
  readonly engine = inject(EditorEngineService);
  readonly state = inject(EditorStateService);

  readonly activeTexLayer = computed(() => this.engine.status()?.textureLayer ?? 1);
  readonly categories = TEXTURE_CATEGORIES;
  readonly textureLayers = TEXTURE_LAYERS;
  readonly filteredTextureLayers = computed(() => {
    const cat = this.state.textureCategory();
    return cat === 'All'
      ? this.textureLayers
      : this.textureLayers.filter((layer) => layer.category === cat);
  });
}
