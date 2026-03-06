import 'package:flutter/material.dart';

/// Shared placeholder shown when a recipe has no image or image fails to load.
class RecipeImagePlaceholder extends StatelessWidget {
  final double iconSize;
  const RecipeImagePlaceholder({super.key, this.iconSize = 48});

  @override
  Widget build(BuildContext context) => Container(
        color: Theme.of(context).colorScheme.surfaceContainerHighest,
        alignment: Alignment.center,
        child: Icon(
          Icons.restaurant_menu,
          size: iconSize,
          color: Theme.of(context).colorScheme.onSurfaceVariant,
        ),
      );
}

/// Shared helper: Image.network with consistent error/loading handling.
Widget recipeNetworkImage(String url, {BoxFit fit = BoxFit.cover, double iconSize = 48}) {
  return Image.network(
    url,
    fit: fit,
    errorBuilder: (_, __, ___) => RecipeImagePlaceholder(iconSize: iconSize),
    frameBuilder: (context, child, frame, wasSync) {
      if (wasSync || frame != null) return child;
      return RecipeImagePlaceholder(iconSize: iconSize);
    },
  );
}

/// Recipe card with image on top and title below.
///
/// When [width] is null the card expands to fill its parent (grid cell).
/// Pass a fixed [width] for mini previews (e.g. in the day-picker sheet).
/// The [onAssign] calendar button is hidden when [width] is set (mini mode).
class RecipeCard extends StatelessWidget {
  final String title;
  final String? imageUrl;
  final VoidCallback? onOpen;
  final VoidCallback? onAssign;

  /// Fixed width for mini-card mode; null = expand to fill parent.
  final double? width;

  const RecipeCard({
    super.key,
    required this.title,
    required this.imageUrl,
    this.onOpen,
    this.onAssign,
    this.width,
  });

  bool get _mini => width != null;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final radius = _mini ? 10.0 : 16.0;
    final titlePad = _mini ? 6.0 : 12.0;
    final titleStyle = _mini
        ? theme.textTheme.bodySmall
        : theme.textTheme.titleMedium;

    Widget card = Card(
      clipBehavior: Clip.antiAlias,
      elevation: 2,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(radius)),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Expanded(
            child: Stack(
              fit: StackFit.expand,
              children: [
                imageUrl == null
                    ? RecipeImagePlaceholder(iconSize: _mini ? 28 : 48)
                    : recipeNetworkImage(imageUrl!, iconSize: _mini ? 28 : 48),
                if (!_mini && onAssign != null)
                  Positioned(
                    top: 8,
                    right: 8,
                    child: Material(
                      color: Colors.black54,
                      shape: const CircleBorder(),
                      child: IconButton(
                        tooltip: 'Assign to day',
                        icon: const Icon(
                          Icons.event,
                          size: 20,
                          color: Colors.white,
                        ),
                        onPressed: onAssign,
                        splashRadius: 22,
                      ),
                    ),
                  ),
              ],
            ),
          ),
          Padding(
            padding: EdgeInsets.all(titlePad),
            child: Text(
              title,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              textAlign: TextAlign.center,
              style: titleStyle,
            ),
          ),
        ],
      ),
    );

    if (_mini) {
      card = SizedBox(
        width: width,
        child: AspectRatio(aspectRatio: 3 / 4, child: card),
      );
    }

    return InkWell(
      onTap: onOpen,
      borderRadius: BorderRadius.circular(radius),
      child: card,
    );
  }

}
