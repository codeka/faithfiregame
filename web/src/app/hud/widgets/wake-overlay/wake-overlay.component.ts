import { Component, input, output } from '@angular/core';
import { WakeOverlayState } from '../../hud.types';

@Component({
  selector: 'app-hud-wake-overlay',
  standalone: true,
  templateUrl: './wake-overlay.component.html',
  styleUrl: './wake-overlay.component.css',
})
export class WakeOverlayComponent {
  readonly state = input.required<WakeOverlayState>();
  readonly finished = output<void>();

  onOverlayEnd(event: Event): void {
    if (event.target === event.currentTarget) {
      this.finished.emit();
    }
  }
}
