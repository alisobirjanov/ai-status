# Pulse for Windows — changes

Each release needs an entry here before its version reaches main: merging
releases it, the release workflow refuses a version without one, and its
words are what installed copies show when they offer the update. Russian
first, then English.

## 0.1.8

**Русский**

- Почта аккаунтов скрыта: видно только начало, например sa****@gmail.com. Щёлкните по ней в настройках, на странице «Аккаунты», чтобы увидеть её целиком, и ещё раз, чтобы скрыть. Удобно, когда ваш экран видят другие.

**English**

- Account emails are hidden, showing only the start, like sa****@gmail.com. Click one on the Accounts page in Settings to see it whole, and again to hide it. Handy when others can see your screen.

## 0.1.7

**Русский**

- Несколько аккаунтов Claude. Добавьте их в настройках, на новой странице «Аккаунты»: нажмите «Добавить аккаунт» и войдите в браузере — набирать команды не нужно. Для каждого аккаунта видны 5-часовой и недельный лимиты, когда они сбросятся и какой из аккаунтов сейчас в Claude Code.
- «Использовать в Claude Code»: один щелчок — и Claude Code работает от этого аккаунта везде, в терминале и в VS Code, без /login. Новые окна Claude Code сразу используют его, уже открытые перезапустите.
- На панели — один аккаунт Claude: тот, что сейчас в Claude Code, тот, у которого больше всего запаса, или все по очереди. Карточка при наведении показывает все.
- Когда у текущего аккаунта кончается лимит, Pulse подскажет, у какого ещё есть запас.
- Если сохранённый вход истёк, Pulse просит Claude Code обновить его, и цифры приходят дальше, даже когда Claude Code не открыт.

**English**

- Several Claude accounts. Add them on the new Accounts page in Settings: click Add account and sign in in your browser — no commands to type. Each one shows its 5-hour and weekly limits, when they reset, and which account Claude Code is in.
- Use in Claude Code: one click, and Claude Code uses that account everywhere — in a terminal, in VS Code — without /login. New Claude Code windows use it straight away; restart the ones already open.
- The rail shows one Claude account: the one in use, the one with the most room, or each in turn. The card on hover lists them all.
- When the account in use runs out of a limit, Pulse tells you which one still has room.
- When a saved login has expired, Pulse asks Claude Code to renew it, so the figures keep coming even when Claude Code isn't open.

## 0.1.6

**Русский**

- Стеклянная панель: сквозь неё видно то, что под ней. Включается в настройках, в «Основных» → «Оформление», там же выбирается прозрачность — 25, 50 или 75%. По умолчанию выключена.
- Карточка при наведении остаётся плотной, чтобы её было легко читать.

**English**

- A glass rail: what is behind it shows through. Switch it on in Settings > General > Appearance and choose how see-through it is — 25, 50 or 75%. It's off by default.
- The card on hover stays solid, so it is easy to read.

## 0.1.5

**Русский**

- Панель прилипает и к верхнему краю экрана. Перетащите её наверх — она повернётся набок: кольца встанут в ряд, значения — рядом с ними, а карточка будет открываться снизу. «Прятать у края экрана» работает и там.
- Края притягивают панель, как магнит. Поднесите её к краю — она прилипнет ещё до того, как вы её отпустите, и дальше будет скользить вдоль края за мышью. Оторвётся она, только если отвести её подальше.
- Всё это анимировано: панель плавно сливается с краем и отделяется от него, поворачивается по пути наверх и обратно и мягко встаёт на место.
- Карточка открывается плавнее: она вырастает из кольца, на которое вы навели мышь, переезжает от кольца к кольцу, когда вы ведёте мышь вдоль панели, и плавно гаснет, когда мышь уходит.
- Исправлено: поверх панели мог появиться заголовок окна или рамка в старом стиле Windows и так и остаться — после щелчка правой кнопкой или когда мышь быстро проходила над панелью.

**English**

- The panel docks at the top of the screen too. Drag it to the top edge and it turns on its side: the rings sit in a row with their figures beside them, and the card opens below. "Tuck away at the edge" works there as well.
- Edges pull the panel in like a magnet. Carry it close to one and it docks before you let go, then slides along the edge with the mouse. It comes off only when you pull it well away.
- Docking is animated. The panel melts into the edge and peels away from it, turns smoothly on its way to or from the top, and glides into place.
- The card opens more smoothly. It grows out from the ring you point at, glides from one ring to the next as you move along the panel, and fades away when you move off.
- Fixed: a title bar or an old-style Windows frame could appear over the panel and stay there, after a right-click or when the mouse passed over the panel quickly.

## 0.1.4

**Русский**

- Светлая тема. В настройках, в «Основных», появился раздел «Оформление» с выбором темы: «Системная», «Светлая» или «Тёмная». Ей следуют окно настроек, панель и её карточка.
- «Системная» повторяет светлый или тёмный режим Windows и меняется вместе с ним. Она выбрана по умолчанию: если Windows в светлом режиме, после обновления Pulse станет светлым. Чтобы он остался тёмным, выберите «Тёмная».
- Кнопка с солнцем или луной в правом верхнем углу настроек переключает тему со светлой на тёмную и обратно одним щелчком.
- С переключателем «Оставлять панель тёмной» панель и её карточка в светлой теме остаются тёмными — так их видно на любых обоях. По умолчанию он выключен.

**English**

- Light mode. Settings > General has a new "Appearance" section with a choice of theme: System, Light or Dark. Settings, the panel and its card all follow it.
- System follows Windows' light or dark mode and changes when Windows does. It's the default, so if Windows is in light mode, Pulse turns light with this update. To keep it dark, pick Dark.
- A button with a sun or moon at the top right of Settings switches between light and dark in one click.
- "Keep the rail dark" leaves the panel and its card dark in light mode, so they read over any wallpaper. It's off by default.

## 0.1.3

**Русский**

- Цвета колец — как в выборе «Кольца показывают» в настройках: 5-часовой лимит оранжевый, недельный — фиолетовый. Когда лимит переходит порог «Красный цвет с», кольцо краснеет.
- Прилипшая к краю панель больше не прячется сама: она всегда открыта. Сворачивание в полоску с выездом при наведении включается в настройках — «Прятать у края экрана».
- У панели больше нет тени: вокруг колец не остаётся тёмного ореола.

**English**

- Rings take the colours of the "Rings show" choices in Settings: the 5-hour limit orange, the weekly one violet. A limit past "Turn red from" turns its ring red.
- A panel docked at an edge no longer tucks itself away: it stays open. Shrinking to a sliver that slides out on hover is a switch in Settings, "Tuck away at the edge".
- The panel no longer casts a shadow, so no dark halo is left round the rings.

## 0.1.2

**Русский**

- Панель прилипает к краю экрана: перетащите её к левому или правому краю, и она встанет вплотную, плавно переходя в край.
- У края панель прячется в тонкую полоску и выезжает, когда вы подводите к ней мышь. Если лимит перешёл красную черту, полоска окрашивается в его цвет.
- Отведите панель от края — и она снова свободная капсула, которая не прячется.
- Уже стоящую панель один раз нужно дотащить до края, чтобы она прилипла.
- В настройках, в «Основных», появились переключатели «Прятать у края экрана» и «Карточка при наведении». Оба включены.

**English**

- The panel docks to a screen edge: drag it to the left or right side and it sits flush against it, flowing into the edge.
- Docked, it tucks away into a thin sliver and slides out when you point at it. When a limit is past the red line, the sliver takes its colour.
- Drag it off the edge and it is a floating capsule again that never tucks away.
- A panel already placed needs dragging to the edge once to dock.
- Settings > General has two new switches, "Tuck away at the edge" and "Card on hover". Both are on.

## 0.1.1

**Русский**

- Новые настройки: четыре страницы — «Основные», «Оповещения», «Клавиши» и «О программе».
- В «Основных» рядом с настройками — сама панель: она меняется сразу, как только вы что-то переключаете.
- «О программе»: обновления, что нового в версии, к чему обращается Pulse, ссылки и кнопка «Сбросить все настройки». Включённые сервисы при сбросе остаются включёнными.
- У каждого аккаунта видно, из какого файла входа Pulse его читает.
- «Оповещения» и «Клавиши» пока не работают: их страницы показывают, как они будут выглядеть.
- Если Windows не на русском и не на английском, даты и время теперь целиком по-английски, а не вперемешку с языком Windows.

**English**

- New Settings in four pages: General, Alerts, Shortcuts and About.
- General shows the panel itself beside the controls, and it changes the moment you do.
- About has updates, what's new, what Pulse talks to, links, and Reset all settings. Services you switched on stay on after a reset.
- Each account says which login file Pulse reads it from.
- Alerts and Shortcuts aren't built yet: their pages show how they will look.
- On Windows in a language other than English or Russian, dates and times are now wholly in English instead of mixed with the Windows language.

## 0.1.0

**Русский**

- Первая версия Pulse для Windows. Плавающая панель с кольцами расхода Claude Code и Codex; при наведении на кольцо — карточка со всеми лимитами и временем сброса.
- Кольца показывают лимит, ближайший к исчерпанию, 5-часовой, недельный или оба сразу.
- Значок в трее показывает и прячет панель; правый щелчок по панели открывает меню.
- Pulse сам проверяет обновления и сообщает уведомлением о новой версии. Устанавливается она из меню в трее или в настройках.

**English**

- The first Pulse for Windows. A floating panel with usage rings for Claude Code and Codex; hover a ring for a card with every limit and when it resets.
- Rings show the limit closest to running out, the 5-hour one, the weekly one, or both.
- The tray icon shows and hides the panel; right-click the panel for its menu.
- Pulse checks for updates itself and says so with a notification when a new version is out. Install it from the tray menu or from Settings.
