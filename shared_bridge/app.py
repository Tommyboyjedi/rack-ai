"""Entry point: python3 -m shared_bridge.app --config /path/to/config.json."""
import argparse
import logging
import sys

from .api import build_server
from .config import Config
from .manager import ReservationManager

LOG = logging.getLogger("shared_bridge")


def main(argv=None):
    parser = argparse.ArgumentParser(prog="shared-bridge")
    parser.add_argument("--config", required=True)
    parser.add_argument("--check", action="store_true", help="validate configuration and exit")
    arguments = parser.parse_args(argv)
    config = Config.load(arguments.config)
    if arguments.check:
        print("SHARED_BRIDGE_CONFIG_VALID")
        return 0
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    manager = ReservationManager(config)
    manager.reconcile()
    server = build_server(config, manager)
    LOG.info("shared-bridge listening on %s:%s", config.listen_host, config.listen_port)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
