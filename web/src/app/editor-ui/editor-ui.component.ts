import { Component, inject, OnInit } from '@angular/core';
import { EngineService } from '../engine.service';
import { CreateAreaModalComponent } from './create-area-modal/create-area-modal.component';
import { EditorStateService } from './editor-state.service';
import { EditorToolbarComponent } from './toolbar/editor-toolbar.component';
import { DecorationToolComponent } from './tools/decoration-tool/decoration-tool.component';
import { HeightmapToolComponent } from './tools/heightmap-tool/heightmap-tool.component';
import { TextureToolComponent } from './tools/texture-tool/texture-tool.component';

@Component({
  selector: 'app-editor-ui',
  standalone: true,
  imports: [
    EditorToolbarComponent,
    HeightmapToolComponent,
    TextureToolComponent,
    DecorationToolComponent,
    CreateAreaModalComponent,
  ],
  providers: [EditorStateService],
  templateUrl: './editor-ui.component.html',
  styleUrl: './editor-ui.component.css',
})
export class EditorUiComponent implements OnInit {
  readonly engine = inject(EngineService);
  readonly state = inject(EditorStateService);

  async ngOnInit(): Promise<void> {
    await this.state.initData();
  }
}
