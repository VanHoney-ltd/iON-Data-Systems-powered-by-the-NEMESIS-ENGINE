export namespace main {
	
	export class BootstrapConfig {
	    serverUrl: string;
	
	    static createFrom(source: any = {}) {
	        return new BootstrapConfig(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.serverUrl = source["serverUrl"];
	    }
	}

}

