import { Component, inject, OnInit } from '@angular/core';
import { EditorEngineService } from './editor-engine.service';
import { CreateAreaModalComponent } from './create-area-modal/create-area-modal.component';
import { EditorStateService } from './editor-state.service';
import { EditorToolbarComponent } from './toolbar/editor-toolbar.component';
import {
  DECORATION_SUBMODES,
  DecorationToolComponent,
} from './tools/decoration-tool/decoration-tool.component';
import {
  HEIGHT_MODES,
  HeightmapToolComponent,
} from './tools/heightmap-tool/heightmap-tool.component';
import { ROAD_SUBMODES, RoadToolComponent } from './tools/road-tool/road-tool.component';
import {
  TEXTURE_LAYERS,
  TextureToolComponent,
} from './tools/texture-tool/texture-tool.component';

const NON_TEXT_INPUT_TYPES = new Set(['range', 'checkbox', 'radio', 'button']);

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) {
    return false;
  }
  if (target instanceof HTMLElement && target.isContentEditable) {
    return true;
  }
  const tag = target.tagName.toUpperCase();
  if (tag === 'TEXTAREA' || tag === 'SELECT') {
    return true;
  }
  if (tag === 'INPUT') {
    const type = (target.getAttribute('type') ?? '').toLowerCase();
    return !NON_TEXT_INPUT_TYPES.has(type);
  }
  return false;
}

@Component({
  selector: 'app-editor-ui',
  standalone: true,
  imports: [
    EditorToolbarComponent,
    HeightmapToolComponent,
    TextureToolComponent,
    RoadToolComponent,
    DecorationToolComponent,
    CreateAreaModalComponent,
  ],
  providers: [EditorStateService],
  templateUrl: './editor-ui.component.html',
  styleUrl: './editor-ui.component.css',
  host: {
    '(window:keydown)': 'onWindowKeyDown($event)',
  },
})
export class EditorUiComponent implements OnInit {
  readonly engine = inject(EditorEngineService);
  readonly state = inject(EditorStateService);

  async ngOnInit(): Promise<void> {
    await this.state.initData();
  }

  onWindowKeyDown(ev: KeyboardEvent): void {
    if (ev.ctrlKey || ev.metaKey || ev.altKey || ev.shiftKey || ev.repeat) {
      return;
    }
    if (this.state.showNewAreaModal() || isEditableTarget(ev.target)) {
      return;
    }
    if (ev.key < '1' || ev.key > '9') {
      return;
    }

    const idx = Number(ev.key) - 1;
    switch (this.state.activeTool()) {
      case 'heightmap': {
        const mode = HEIGHT_MODES[idx];
        if (mode) this.engine.setHeightMode(mode.id);
        break;
      }
      case 'texture': {
        const layer = TEXTURE_LAYERS[idx];
        if (layer) this.engine.setTextureLayer(layer.index);
        break;
      }
      case 'road': {
        const submode = ROAD_SUBMODES[idx];
        if (submode) this.engine.setRoadSubmode(submode.id);
        break;
      }
      case 'decoration': {
        const submode = DECORATION_SUBMODES[idx];
        if (submode) this.engine.setDecorationSubmode(submode.id);
        break;
      }
    }
  }
}
