import 'package:flutter/material.dart';

class RecipeThumbTile extends StatelessWidget {
  final double width;
  final double height;
  final String title;
  final String? imageUrl;
  final VoidCallback onOpen;
  final VoidCallback onDelete;

  const RecipeThumbTile({
    super.key,
    required this.width,
    required this.height,
    required this.title,
    required this.imageUrl,
    required this.onOpen,
    required this.onDelete,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return SizedBox(
      width: width,
      height: height,
      child: Material(
        color: theme.colorScheme.surface,
        elevation: 1,
        borderRadius: BorderRadius.circular(12),
        clipBehavior: Clip.antiAlias, // prevents paint overflow
        child: InkWell(
          onTap: onOpen,
          child: Stack(
            children: [
              // Content
              Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  // Image fills available vertical space first
                  Expanded(
                    flex: 7,
                    child: imageUrl != null && imageUrl!.isNotEmpty
                        ? Image.network(imageUrl!, fit: BoxFit.cover)
                        : Container(
                            color: theme.colorScheme.surfaceContainerHighest,
                            alignment: Alignment.center,
                            child: Icon(
                              Icons.restaurant_menu,
                              size: 48,
                              color: theme.colorScheme.onSurfaceVariant,
                            ),
                          ),
                  ),
                  // Title area (constrained)
                  Padding(
                    padding: const EdgeInsets.fromLTRB(8, 6, 8, 6),
                    child: Text(
                      title,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      textAlign: TextAlign.center,
                      style: theme.textTheme.bodyMedium?.copyWith(
                        height: 1.2, // tighter, avoids baseline spill
                      ),
                    ),
                  ),
                ],
              ),

              // Delete button overlay (kept from your original behavior)
              Positioned(
                right: 4,
                top: 4,
                child: IconButton(
                  visualDensity: VisualDensity.compact,
                  style: IconButton.styleFrom(
                    backgroundColor: theme.colorScheme.surface.withValues(
                      alpha: 0.6,
                    ),
                  ),
                  icon: const Icon(Icons.close, size: 16),
                  onPressed: onDelete,
                  tooltip: 'Remove',
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
