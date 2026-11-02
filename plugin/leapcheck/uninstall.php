<?php
/**
 * Uninstall: remove all LeapCheck options and cached results.
 *
 * @package LeapCheck
 */

if ( ! defined( 'WP_UNINSTALL_PLUGIN' ) ) {
	exit;
}

/**
 * Delete this site's LeapCheck data.
 */
function leapcheck_uninstall_site() {
	$keys = get_option( 'leapcheck_cache_keys', array() );
	if ( is_array( $keys ) ) {
		foreach ( $keys as $key ) {
			delete_transient( (string) $key );
		}
	}
	delete_option( 'leapcheck_cache_keys' );
	delete_option( 'leapcheck_consent' );
}

if ( is_multisite() ) {
	$leapcheck_site_ids = get_sites(
		array(
			'fields' => 'ids',
			'number' => 0,
		)
	);
	foreach ( $leapcheck_site_ids as $leapcheck_site_id ) {
		switch_to_blog( (int) $leapcheck_site_id );
		leapcheck_uninstall_site();
		restore_current_blog();
	}
} else {
	leapcheck_uninstall_site();
}
