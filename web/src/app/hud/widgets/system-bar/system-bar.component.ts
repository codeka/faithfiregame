import { Component, input, output } from '@angular/core';
import { RouterLink } from '@angular/router';
import { HudButtonComponent } from '../../primitives/hud-button/hud-button.component';

@Component({
  selector: 'app-hud-system-bar',
  standalone: true,
  imports: [RouterLink, HudButtonComponent],
  templateUrl: './system-bar.component.html',
  styleUrl: './system-bar.component.css',
})
export class SystemBarComponent {
  readonly showEditor = input<boolean>(false);
  readonly inventoryOpen = input<boolean>(false);
  readonly leave = output<void>();
  readonly toggleInventory = output<void>();
}
