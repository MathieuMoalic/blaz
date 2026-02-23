import 'package:flutter/material.dart';

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
                    ? _placeholder(_mini ? 28 : 48)
                    : Image.network(
                        imageUrl!,
                        fit: BoxFit.cover,
                        frameBuilder: (context, child, frame, wasSync) {
                          if (wasSync || frame != null) return child;
                          return const Center(child: CircularProgressIndicator());
                        },
                        errorBuilder: (_, __, ___) =>
                            _placeholder(_mini ? 28 : 48),
                      ),
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

  Widget _placeholder(double iconSize) => Container(
        alignment: Alignment.center,
        child: Icon(Icons.restaurant_menu, size: iconSize),
      );
}
