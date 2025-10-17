import 'package:flutter/material.dart';

class AppTitle extends StatelessWidget {
  final String text;
  final Widget? trailing;
  const AppTitle(this.text, {super.key, this.trailing});

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 56,
      alignment: Alignment.centerLeft,
      padding: const EdgeInsets.symmetric(horizontal: 12),
      child: Row(
        children: [
          Expanded(
            child: Text(text, style: Theme.of(context).textTheme.titleLarge),
          ),
          if (trailing != null) trailing!,
        ],
      ),
    );
  }
}
