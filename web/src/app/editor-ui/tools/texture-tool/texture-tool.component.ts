import { Component, computed, inject } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { DEFAULT_TEXTURE_LAYERS, EditorEngineService } from '../../editor-engine.service';
import { EditorStateService } from '../../editor-state.service';

export const TEXTURE_LAYERS = DEFAULT_TEXTURE_LAYERS;

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
  readonly categories = this.state.textureCategories;
  readonly textureLayers = this.state.terrainTextureLayers;
  readonly filteredTextureLayers = computed(() => {
    const cat = this.state.textureCategory();
    const layers = this.textureLayers();
    return cat === 'All' ? layers : layers.filter((layer) => layer.category === cat);
  });
}
