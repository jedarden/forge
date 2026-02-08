"""
FORGE CLI Entry Point
"""

from forge.app import ForgeApp


def main() -> None:
    """Main entry point for the CLI"""
    app = ForgeApp()
    app.run()


if __name__ == "__main__":
    main()
