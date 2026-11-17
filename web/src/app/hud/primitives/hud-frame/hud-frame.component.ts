import { Component, input } from '@angular/core';
import { HudFrameVariant, HudTone } from '../../hud.types';

@Component({
  selector: 'app-hud-frame',
  standalone: true,
  templateUrl: './hud-frame.component.html',
  styleUrl: './hud-frame.component.css',
  host: {
    '[attr.data-hud-tone]': 'tone()',
    '[attr.data-hud-variant]': 'variant()',
    '[class.has-top-dots]': 'topDots()',
    '[class.has-bottom-dots]': 'bottomDots()',
  },
})
export class HudFrameComponent {
  readonly variant = input<HudFrameVariant>('minimal');
  readonly tone = input<HudTone>('cyan');
  readonly topDots = input<boolean>(false);
  readonly bottomDots = input<boolean>(false);
  readonly topFlare = input<boolean>(false);
  readonly edgeAccents = input<boolean>(true);
}
