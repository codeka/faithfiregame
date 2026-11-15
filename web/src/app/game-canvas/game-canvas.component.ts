import { AfterViewInit, Component, ElementRef, inject, ViewChild } from '@angular/core';
import { EngineService } from '../engine.service';

@Component({
  selector: 'app-game-canvas',
  standalone: true,
  template: `<canvas #viewportCanvas class="viewport-canvas"></canvas>`,
  styles: [
    `
      :host {
        display: block;
        width: 100vw;
        height: 100vh;
      }
      .viewport-canvas {
        display: block;
        width: 100%;
        height: 100%;
        outline: none;
        cursor: crosshair;
      }
    `,
  ],
})
export class GameCanvasComponent implements AfterViewInit {
  @ViewChild('viewportCanvas', { static: true })
  private readonly canvasRef!: ElementRef<HTMLCanvasElement>;

  private readonly engine = inject(EngineService);

  ngAfterViewInit(): void {
    void this.engine.init(this.canvasRef.nativeElement);
  }
}
