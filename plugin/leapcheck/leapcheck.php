<?php
/**
 * Plugin Name:       LeapCheck – PHP Upgrade Checker
 * Plugin URI:        https://leapcheck.vercel.app
 * Description:       Check before you leap: see which of your plugins and themes are ready for PHP 8.3, 8.4 and 8.5, including old libraries hidden inside them.
 * Version:           0.1.0
 * Requires at least: 5.8
 * Requires PHP:      7.4
 * Author:            LeapCheck
 * Author URI:        https://leapcheck.vercel.app
 * License:           GPL-2.0-or-later
 * License URI:       https://www.gnu.org/licenses/gpl-2.0.html
 * Text Domain:       leapcheck
 *
 * @package LeapCheck
 */

if ( ! defined( 'ABSPATH' ) ) {
	exit;
}

define( 'LEAPCHECK_VERSION', '0.1.0' );
define( 'LEAPCHECK_FILE', __FILE__ );

if ( ! defined( 'LEAPCHECK_BASE_URL' ) ) {
	// Can be overridden in wp-config.php or with the `leapcheck_base_url` filter.
	define( 'LEAPCHECK_BASE_URL', 'https://leapcheck.vercel.app' );
}

require_once __DIR__ . '/includes/functions.php';

if ( is_admin() ) {
	require_once __DIR__ . '/includes/admin.php';
}
