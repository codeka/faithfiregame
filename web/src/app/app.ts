import { Component } from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { GameCanvasComponent } from './game-canvas/game-canvas.component';

@Component({
  selector: 'app-root',
  imports: [RouterOutlet, GameCanvasComponent],
  templateUrl: './app.html',
  styleUrl: './app.css',
})
export class App {}
